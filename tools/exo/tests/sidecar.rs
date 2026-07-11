#![allow(clippy::disallowed_methods)]

use serde_json::Value as JsonValue;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const SIDECAR_GIT_USER_NAME: &str = "Exosuit";
const SIDECAR_GIT_USER_EMAIL: &str = "exo@exosuit.local";

fn git_config_identity(root: &Path) {
    for args in [
        ["config", "user.email", "exo-tests@example.invalid"],
        ["config", "user.name", "Exo Tests"],
    ] {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("run git config");

        assert!(
            output.status.success(),
            "git config failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

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
    git_config_identity(root);
    git_success(root, &["branch", "-M", "main"]);
}

fn git_init_bare(root: &Path) {
    let output = Command::new("git")
        .args(["init", "--bare"])
        .current_dir(root)
        .output()
        .expect("run git init --bare");

    assert!(
        output.status.success(),
        "git init --bare failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("run git");

    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git stdout is utf-8")
}

fn git_local_config(root: &Path, key: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "--local", "--get", key])
        .current_dir(root)
        .output()
        .expect("run git config --local");
    if output.status.success() {
        let value = String::from_utf8(output.stdout).expect("git config stdout is utf-8");
        let value = value.trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn git_success(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("run git");

    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn seeded_sidecar_remote(temp: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
    let remote = temp.join(format!("{name}.git"));
    let seeder = temp.join(format!("{name}-seeder"));
    std::fs::create_dir_all(&remote).expect("create bare remote dir");
    git_init_bare(&remote);
    git_success(
        temp,
        &[
            "clone",
            remote.to_str().expect("remote path is utf-8"),
            seeder.to_str().expect("seeder path is utf-8"),
        ],
    );
    git_config_identity(&seeder);
    git_success(&seeder, &["checkout", "-B", "main"]);

    if files.is_empty() {
        std::fs::write(seeder.join("README.md"), "sidecar hub\n").expect("write sidecar README");
    } else {
        for (relative, contents) in files {
            let path = seeder.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create sidecar remote file parent");
            }
            std::fs::write(path, contents).expect("write sidecar remote file");
        }
    }

    git_success(&seeder, &["add", "-A"]);
    git_success(&seeder, &["commit", "-m", "Seed sidecar hub"]);
    git_success(&seeder, &["push", "-u", "origin", "main"]);
    git_success(&remote, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    remote
}

fn git_status_porcelain(root: &Path) -> String {
    let output = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(root)
        .output()
        .expect("run git status");

    assert!(
        output.status.success(),
        "git status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git status stdout is utf-8")
}

fn exo_cmd(root: &Path, home: &Path, config_home: &Path) -> assert_cmd::Command {
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("exo");
    cmd.current_dir(root)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", config_home);
    if cfg!(windows) {
        cmd.env("USERPROFILE", home).env("APPDATA", config_home);
    }
    cmd
}

fn exo_direct_cmd(root: &Path, home: &Path, config_home: &Path) -> assert_cmd::Command {
    // Fixture assertions inspect local state without exercising daemon lifecycle.
    let mut cmd = exo_cmd(root, home, config_home);
    cmd.arg("--direct");
    cmd
}

fn add_git_remote_rewrite(cmd: &mut assert_cmd::Command, remote_url: &str, local_remote: &Path) {
    cmd.env("GIT_CONFIG_COUNT", "1")
        .env(
            "GIT_CONFIG_KEY_0",
            format!(
                "url.{}.insteadOf",
                local_remote.to_str().expect("local remote path is utf-8")
            ),
        )
        .env("GIT_CONFIG_VALUE_0", remote_url);
}

fn short_tempdir() -> tempfile::TempDir {
    let parent = if cfg!(windows) {
        std::env::temp_dir()
    } else {
        PathBuf::from("/tmp")
    };
    std::fs::create_dir_all(&parent).expect("create sidecar temp parent");
    tempfile::Builder::new()
        .prefix("exo-sidecar-")
        .tempdir_in(parent)
        .expect("create tempdir")
}

fn json_result(output: &[u8]) -> JsonValue {
    let envelope: JsonValue = serde_json::from_slice(output).expect("valid json envelope");
    assert_eq!(envelope["status"], "ok");
    envelope["result"].clone()
}

fn json_error(output: &[u8]) -> JsonValue {
    let envelope: JsonValue = serde_json::from_slice(output).expect("valid json envelope");
    assert_eq!(envelope["status"], "error");
    envelope["error"].clone()
}

fn project_state_root(sidecar_root: &Path, key: &str) -> PathBuf {
    sidecar_root.join("projects").join(key)
}

fn project_state_path(sidecar_root: &Path, key: &str, relative: &[&str]) -> PathBuf {
    let mut path = project_state_root(sidecar_root, key);
    for component in relative {
        path.push(component);
    }
    path
}

fn policy_value_for_sidecar_key(config_home: &Path, sidecar_key: &str, key: &str) -> String {
    let policy_path = config_home.join("exo").join("projects.toml");
    let policy = std::fs::read_to_string(&policy_path).expect("read project policy");
    let parsed: toml::Value = toml::from_str(&policy).expect("project policy is valid TOML");
    let projects = parsed["projects"]
        .as_table()
        .unwrap_or_else(|| panic!("missing projects table in {policy_path:?}"));
    for project in projects.values() {
        if project["sidecar_key"].as_str() == Some(sidecar_key) {
            return project[key]
                .as_str()
                .unwrap_or_else(|| {
                    panic!("missing {key} for sidecar {sidecar_key} in {policy_path:?}")
                })
                .to_string();
        }
    }
    panic!("missing sidecar {sidecar_key} in {policy_path:?}")
}

fn sidecar_manifest_project_id(sidecar_root: &Path, key: &str) -> String {
    let manifest_path = project_state_path(sidecar_root, key, &["sidecar.toml"]);
    let manifest = std::fs::read_to_string(&manifest_path).expect("read sidecar manifest");
    let parsed: toml::Value = toml::from_str(&manifest).expect("sidecar manifest is valid TOML");
    parsed["sidecar"]["project_id"]
        .as_str()
        .unwrap_or_else(|| panic!("missing sidecar.project_id in {manifest_path:?}"))
        .to_string()
}

fn project_id_for(root: &Path, home: &Path, config_home: &Path) -> String {
    let output = exo_cmd(root, home, config_home)
        .args(["--format", "json", "project", "resolve"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    result["project"]["id"]
        .as_str()
        .expect("project id")
        .to_string()
}

fn policy_contains_project_id(config_home: &Path, project_id: &str) -> bool {
    let policy_path = config_home.join("exo").join("projects.toml");
    let policy = std::fs::read_to_string(&policy_path).expect("read project policy");
    let parsed: toml::Value = toml::from_str(&policy).expect("project policy is valid TOML");
    parsed["projects"]
        .as_table()
        .expect("projects table")
        .contains_key(project_id)
}

fn sidecar_workspace_roots(
    sidecar_root: &Path,
    key: &str,
    table: &str,
    column: &str,
) -> Vec<String> {
    let db_path = project_state_path(sidecar_root, key, &["cache", "exo.db"]);
    let db = exosuit_storage::open_database(&db_path).expect("open sidecar db");
    let sql = format!(
        "SELECT DISTINCT {column}
         FROM {table}
         WHERE {column} IS NOT NULL
         ORDER BY {column}"
    );
    let mut stmt = db.connection().prepare(&sql).expect("prepare roots query");
    stmt.query_map([], |row| row.get::<_, String>(0))
        .expect("query roots")
        .collect::<Result<Vec<_>, _>>()
        .expect("read roots")
}

fn test_workspace_id(project_id: &str, workspace_root: &Path) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(project_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(workspace_root.to_string_lossy().as_bytes());
    let hash = hasher.finalize().to_hex();
    format!("workspace:{project_id}:{}", &hash.as_str()[..16])
}

fn phase_owner_workspace_ids(
    sidecar_root: &Path,
    key: &str,
    workspace_root: &Path,
) -> (String, String) {
    let db_path = project_state_path(sidecar_root, key, &["cache", "exo.db"]);
    let db = exosuit_storage::open_database(&db_path).expect("open sidecar db");
    db.connection()
        .query_row(
            "SELECT owner_id, claimed_by_workspace_id
             FROM phase_ownership_data
             WHERE claimed_by_workspace_root = ?1
             ORDER BY phase_id
             LIMIT 1",
            [workspace_root.to_string_lossy().as_ref()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .expect("read phase owner workspace ids")
}

fn seed_sidecar_workspace_root_state(sidecar_root: &Path, key: &str, workspace_root: &Path) {
    let db_path = project_state_path(sidecar_root, key, &["cache", "exo.db"]);
    let db = exosuit_storage::open_database(&db_path).expect("open sidecar db");
    let conn = db.connection();
    conn.execute(
        "INSERT INTO epochs (text_id, title, slug, reviewed, sort_key)
         VALUES ('move-root-epoch', 'Move Root Epoch', NULL, 0, '00000001')",
        [],
    )
    .expect("insert epoch");
    conn.execute(
        "INSERT INTO phases (text_id, title, status, epoch_id, kind, slug, sort_key)
         SELECT 'move-root-phase', 'Move Root Phase', 'in-progress', id, 'regular', NULL, '00000001'
         FROM epochs
         WHERE text_id = 'move-root-epoch'",
        [],
    )
    .expect("insert phase");
    conn.execute(
        "INSERT INTO workspace_active_phase (workspace_root, phase_id, updated_at)
         SELECT ?1, id, '2026-06-22T00:00:00.000Z'
         FROM phases
         WHERE text_id = 'move-root-phase'",
        [workspace_root.to_string_lossy().as_ref()],
    )
    .expect("insert workspace active phase");
    conn.execute(
        "INSERT INTO phase_ownership
         (phase_id, owner_kind, owner_id, claimed_by_workspace_id, claimed_by_workspace_root, claimed_at, updated_at)
         SELECT id, 'workspace', 'workspace-owner', 'workspace-owner', ?1,
                '2026-06-22T00:00:00.000Z', '2026-06-22T00:00:00.000Z'
         FROM phases
         WHERE text_id = 'move-root-phase'",
        [workspace_root.to_string_lossy().as_ref()],
    )
    .expect("insert phase owner");
}

fn create_rfc_00001(root: &Path, home: &Path, config_home: &Path) -> PathBuf {
    let output = exo_direct_cmd(root, home, config_home)
        .args([
            "--format",
            "json",
            "rfc",
            "create",
            "Root Move RFC",
            "--id",
            "00001",
            "--feature",
            "Move Root",
            "--stage",
            "0",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    PathBuf::from(result["path"].as_str().expect("created RFC path"))
}

fn copy_rfc_file_to_new_root(old_rfc_path: &Path, old_root: &Path, new_root: &Path) {
    let relative = if old_rfc_path.is_absolute() {
        let canonical_rfc = old_rfc_path.canonicalize().expect("canonical RFC path");
        let canonical_root = old_root.canonicalize().expect("canonical old root");
        canonical_rfc
            .strip_prefix(canonical_root)
            .expect("RFC path is under old root")
            .to_path_buf()
    } else {
        old_rfc_path.to_path_buf()
    };
    let new_path = new_root.join(&relative);
    std::fs::create_dir_all(new_path.parent().expect("new RFC parent"))
        .expect("create new RFC parent");
    std::fs::copy(old_rfc_path, &new_path).expect("copy RFC to new root");
}

fn daemon_paths(root: &Path) -> Option<(PathBuf, PathBuf)> {
    exo::daemon::paths_for_workspace(root)
        .ok()
        .map(|paths| (paths.pid_path(), paths.socket_path()))
}

fn assert_no_work_repo_daemon_runtime(root: &Path) {
    let runtime_dir = root.join(".exo/runtime");
    assert!(
        !runtime_dir.join("daemon.pid").exists(),
        "work repo should not contain daemon pid at {}",
        runtime_dir.join("daemon.pid").display()
    );
    assert!(
        !runtime_dir.join("daemon.sock").exists(),
        "work repo should not contain daemon socket at {}",
        runtime_dir.join("daemon.sock").display()
    );
}

fn kill_test_daemon_paths(paths: Option<(PathBuf, PathBuf)>) {
    let Some((pid_path, socket_path)) = paths else {
        return;
    };

    if let Ok(pid_str) = std::fs::read_to_string(&pid_path)
        && let Ok(pid) = pid_str.trim().parse::<i32>()
    {
        #[cfg(unix)]
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGTERM,
        );
        #[cfg(windows)]
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = std::fs::remove_file(socket_path);
    let _ = std::fs::remove_file(pid_path);
}

struct DaemonPathGuard {
    paths: Option<(PathBuf, PathBuf)>,
}

impl DaemonPathGuard {
    fn new(root: &Path) -> Self {
        Self {
            paths: daemon_paths(root),
        }
    }
}

impl Drop for DaemonPathGuard {
    fn drop(&mut self) {
        kill_test_daemon_paths(self.paths.clone());
    }
}

fn read_diagnostics_events(path: &Path) -> Vec<JsonValue> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if cfg!(windows) && error.raw_os_error() == Some(33) => return Vec::new(),
        Err(error) => panic!("read diagnostics: {error}"),
    };
    let lines = contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let last_index = lines.len().saturating_sub(1);

    lines
        .into_iter()
        .enumerate()
        .filter_map(|(index, line)| match serde_json::from_str(line) {
            Ok(value) => Some(value),
            Err(_) if index == last_index => None,
            Err(error) => panic!("diagnostics line should be valid json: {error}"),
        })
        .collect()
}

fn wait_for_diagnostics_event(path: &Path, name: &str) -> Vec<JsonValue> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if path.exists() {
            let events = read_diagnostics_events(path);
            if events
                .iter()
                .any(|event| event.get("event").and_then(JsonValue::as_str) == Some(name))
            {
                return events;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    if path.exists() {
        read_diagnostics_events(path)
    } else {
        Vec::new()
    }
}

fn wait_for_daemon_operation(path: &Path, namespace: &str, operation: &str) -> Vec<JsonValue> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if path.exists() {
            let events = read_diagnostics_events(path);
            if events.iter().any(|event| {
                event.get("event").and_then(JsonValue::as_str) == Some("request.invoke_end")
                    && event.get("namespace").and_then(JsonValue::as_str) == Some(namespace)
                    && event.get("operation").and_then(JsonValue::as_str) == Some(operation)
                    && event.get("status").and_then(JsonValue::as_str) == Some("ok")
            }) {
                return events;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    if path.exists() {
        read_diagnostics_events(path)
    } else {
        Vec::new()
    }
}

fn assert_has_daemon_operation(events: &[JsonValue], namespace: &str, operation: &str) {
    assert!(
        events.iter().any(|event| {
            event.get("event").and_then(JsonValue::as_str) == Some("request.invoke_end")
                && event.get("namespace").and_then(JsonValue::as_str) == Some(namespace)
                && event.get("operation").and_then(JsonValue::as_str) == Some(operation)
                && event.get("status").and_then(JsonValue::as_str) == Some("ok")
        }),
        "expected daemon invoke_end for {namespace}.{operation}; got {events:?}"
    );
}

fn link_sidecar(repo: &Path, home: &Path, config_home: &Path, sidecar_root: &Path) {
    link_sidecar_with_key(repo, home, config_home, sidecar_root, "external-test");
}

fn link_sidecar_with_key(
    repo: &Path,
    home: &Path,
    config_home: &Path,
    sidecar_root: &Path,
    key: &str,
) {
    exo_direct_cmd(repo, home, config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "link",
            "--key",
            key,
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ])
        .assert()
        .success();
}

struct UnrelatedSidecarFixture {
    _temp: tempfile::TempDir,
    repo: PathBuf,
    home: PathBuf,
    config_home: PathBuf,
    sidecar_root: PathBuf,
}

struct BasicSidecarFixture {
    _temp: tempfile::TempDir,
    repo: PathBuf,
    home: PathBuf,
    config_home: PathBuf,
    sidecar_root: PathBuf,
}

fn basic_sidecar_fixture() -> BasicSidecarFixture {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    std::fs::write(sidecar_root.join("README.md"), "sidecar\n").expect("write readme");
    std::fs::write(
        sidecar_root.join(".gitignore"),
        "projects/*/cache/\nprojects/*/runtime/\n",
    )
    .expect("write sidecar gitignore");
    git_success(&sidecar_root, &["add", "README.md", ".gitignore"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial sidecar"]);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    if !git_status_porcelain(&sidecar_root).is_empty() {
        git_success(&sidecar_root, &["add", "-A"]);
        git_success(&sidecar_root, &["commit", "-m", "Link sidecar project"]);
    }

    BasicSidecarFixture {
        _temp: temp,
        repo,
        home,
        config_home,
        sidecar_root,
    }
}

fn unrelated_sidecar_fixture() -> UnrelatedSidecarFixture {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let remote = temp.path().join("sidecars.git");
    let seeder = temp.path().join("remote-seeder");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&remote).expect("create remote root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&remote);

    std::fs::write(sidecar_root.join("local.md"), "local\n").expect("write local sidecar file");
    git_success(&sidecar_root, &["add", "local.md"]);
    git_success(&sidecar_root, &["commit", "-m", "Local sidecar root"]);

    git_success(
        temp.path(),
        &[
            "clone",
            remote.to_str().expect("remote path is utf-8"),
            seeder.to_str().expect("seeder path is utf-8"),
        ],
    );
    git_config_identity(&seeder);
    git_success(&seeder, &["checkout", "-B", "main"]);
    std::fs::write(seeder.join("remote.md"), "remote\n").expect("write remote sidecar file");
    git_success(&seeder, &["add", "remote.md"]);
    git_success(&seeder, &["commit", "-m", "Remote sidecar root"]);
    git_success(&seeder, &["push", "origin", "main"]);

    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path is utf-8"),
        ],
    );
    git_success(&sidecar_root, &["fetch", "origin"]);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    std::fs::write(
        sidecar_root.join(".gitignore"),
        "projects/*/cache/\nprojects/*/runtime/\n",
    )
    .expect("write sidecar gitignore");
    git_success(&sidecar_root, &["add", "-A"]);
    git_success(
        &sidecar_root,
        &["commit", "-m", "Link unrelated sidecar project"],
    );

    UnrelatedSidecarFixture {
        _temp: temp,
        repo,
        home,
        config_home,
        sidecar_root,
    }
}

fn sidecar_write_owner_marker_path(sidecar_root: &Path, key: &str) -> PathBuf {
    let mut sanitized = String::new();
    for ch in key.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            sanitized.push(ch);
        } else if !sanitized.ends_with('-') {
            sanitized.push('-');
        }
    }
    let sanitized = sanitized.trim_matches('-');
    let prefix = if sanitized.is_empty() {
        "sidecar"
    } else {
        sanitized
    };
    let digest = blake3::hash(key.as_bytes()).to_hex().to_string();
    sidecar_root
        .join(".git/exo-write-owners")
        .join(format!("{prefix}-{}.json", &digest[..12]))
}

fn write_sidecar_write_owner_marker(
    sidecar_root: &Path,
    key: &str,
    pid: u32,
    workspace_root: &Path,
    executable_blake3: Option<String>,
    process_start_id: Option<String>,
) {
    write_sidecar_write_owner_marker_with_options(
        sidecar_root,
        key,
        pid,
        workspace_root,
        None,
        executable_blake3,
        process_start_id,
        1,
    );
}

fn write_sidecar_write_owner_marker_with_options(
    sidecar_root: &Path,
    key: &str,
    pid: u32,
    workspace_root: &Path,
    executable_path: Option<&Path>,
    executable_blake3: Option<String>,
    process_start_id: Option<String>,
    refreshed_at_ms: u128,
) {
    let state_root = sidecar_root.join("projects").join(key);
    let marker_path = sidecar_write_owner_marker_path(sidecar_root, key);
    std::fs::create_dir_all(marker_path.parent().unwrap()).expect("create owner marker dir");
    let marker = serde_json::json!({
        "version": 1,
        "sidecar_key": key,
        "sidecar_root": sidecar_root,
        "workspace_root": workspace_root,
        "state_root": state_root,
        "db_path": state_root.join("cache/exo.db"),
        "runtime_dir": state_root.join("runtime"),
        "pid": pid,
        "executable_path": executable_path,
        "executable_blake3": executable_blake3,
        "process_start_id": process_start_id,
        "machine": test_machine_identity(),
        "acquired_at_ms": refreshed_at_ms,
        "refreshed_at_ms": refreshed_at_ms,
    });
    std::fs::write(
        marker_path,
        serde_json::to_string_pretty(&marker).expect("serialize owner marker"),
    )
    .expect("write owner marker");
}

fn test_machine_identity() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(unix)]
fn exo_binary_blake3() -> String {
    let path = assert_cmd::cargo::cargo_bin!("exo");
    file_blake3(&path)
}

#[cfg(unix)]
fn file_blake3(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("read exo binary");
    blake3::hash(&bytes).to_hex().to_string()
}

#[cfg(unix)]
fn detached_sleep_pid_hash_and_start_id() -> (u32, String, String) {
    let child = Command::new("sleep")
        .arg("60")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn detached sleep");
    let pid = child.id();
    drop(child);

    let sleep_hash = process_executable_blake3(pid);
    let start_id = process_start_identity(pid);

    (pid, sleep_hash, start_id)
}

#[cfg(unix)]
fn spawned_stale_exo_pid_hash_start_id_and_path(
    temp: &Path,
    repo: &Path,
    home: &Path,
    config_home: &Path,
) -> (std::process::Child, u32, String, String, PathBuf) {
    let stale_bin_dir = temp.join("stale-exo-bin");
    std::fs::create_dir_all(&stale_bin_dir).expect("create stale exo bin dir");
    let stale_exo = stale_bin_dir.join("exo");
    std::fs::copy(assert_cmd::cargo::cargo_bin!("exo"), &stale_exo).expect("copy stale exo");
    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&stale_exo)
            .expect("open stale exo for append");
        file.write_all(b"\n").expect("append stale exo marker byte");
    }
    let mut permissions = std::fs::metadata(&stale_exo)
        .expect("read stale exo metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&stale_exo, permissions).expect("chmod stale exo");

    let child = Command::new(&stale_exo)
        .args(["--direct", "mcp", "worker"])
        .current_dir(repo)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", config_home)
        .env("EXO_NO_REEXEC", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn stale exo worker");
    let pid = child.id();
    let hash = process_executable_blake3(pid);
    let start_id = process_start_identity(pid);
    (child, pid, hash, start_id, stale_exo)
}

#[cfg(target_os = "linux")]
fn process_executable_blake3(pid: u32) -> String {
    let path = format!("/proc/{pid}/exe");
    let bytes = std::fs::read(path).expect("read process executable");
    blake3::hash(&bytes).to_hex().to_string()
}

#[cfg(target_os = "macos")]
fn process_executable_blake3(pid: u32) -> String {
    for _ in 0..20 {
        let output = Command::new("lsof")
            .args(["-p", &pid.to_string(), "-a", "-d", "txt", "-Fn"])
            .output()
            .expect("resolve process path");
        if output.status.success() {
            let stdout = String::from_utf8(output.stdout).expect("process path is utf-8");
            for line in stdout.lines() {
                let Some(path) = line.strip_prefix('n') else {
                    continue;
                };
                let path = PathBuf::from(path);
                if path.is_file() {
                    return file_blake3(&path);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("failed to resolve executable path for process {pid}");
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn process_executable_blake3(_pid: u32) -> String {
    panic!("process executable identity test helper is not supported on this platform");
}

#[cfg(target_os = "linux")]
fn process_start_identity(pid: u32) -> String {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).expect("read process stat");
    let close_paren = stat.rfind(')').expect("process stat has command");
    let start_time_ticks = stat[close_paren + 1..]
        .split_whitespace()
        .nth(19)
        .expect("process stat has start time");
    format!("linux-starttime:{start_time_ticks}")
}

#[cfg(target_os = "macos")]
fn process_start_identity(pid: u32) -> String {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()
        .expect("resolve process start identity");
    assert!(
        output.status.success(),
        "resolve process start identity failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let start = String::from_utf8(output.stdout)
        .expect("process start identity is utf-8")
        .trim()
        .to_string();
    assert!(!start.is_empty(), "process start identity is empty");
    format!("macos-lstart:{start}")
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn process_start_identity(_pid: u32) -> String {
    panic!("process start identity test helper is not supported on this platform");
}

#[cfg(target_os = "linux")]
fn process_is_defunct(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    let Some(close_paren) = stat.rfind(')') else {
        return false;
    };
    stat[close_paren + 1..]
        .split_whitespace()
        .next()
        .is_some_and(|state| matches!(state, "Z" | "X" | "x"))
}

#[cfg(target_os = "macos")]
fn process_is_defunct(pid: u32) -> bool {
    let Ok(output) = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .is_some_and(|state| state.contains('Z'))
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn process_is_defunct(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    if pid == 0 || process_is_defunct(pid) {
        return false;
    }
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(unix)]
fn cleanup_process(pid: u32) {
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    for _ in 0..20 {
        if !process_alive(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

fn exited_child_pid() -> u32 {
    let mut command = if cfg!(windows) {
        let mut command = Command::new("cmd.exe");
        command.args(["/C", "exit", "0"]);
        command
    } else {
        let mut command = Command::new("sh");
        command.args(["-c", "exit 0"]);
        command
    };
    let mut child = command.spawn().expect("spawn short-lived child");
    let pid = child.id();
    child.wait().expect("wait for child");
    pid
}

fn insert_idea(sidecar_root: &Path, text_id: &str, title: &str) {
    insert_idea_for_sidecar_project(sidecar_root, "external-test", text_id, title);
}

fn insert_idea_for_sidecar_project(sidecar_root: &Path, key: &str, text_id: &str, title: &str) {
    let db_path = sidecar_root
        .join("projects")
        .join(key)
        .join("cache")
        .join("exo.db");
    let db = exosuit_storage::open_database(&db_path).expect("open sidecar db");
    db.connection()
        .execute(
            "INSERT INTO ideas
             (text_id, title, description, status, created_at, source)
             VALUES (?1, ?2, NULL, 'new', '2026-06-03T00:00:00Z', 'user')",
            (text_id, title),
        )
        .expect("insert idea");
}

fn append_idea_sql(sidecar_root: &Path, text_id: &str, title: &str) {
    fn sqlite_string_literal(value: &str) -> String {
        value.replace('\'', "''")
    }

    let path = sidecar_root
        .join("projects")
        .join("external-test")
        .join("agent-context")
        .join("ideas.sql");
    let mut contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| "-- Auto-generated by exo. Regenerate: exo status\n".to_string());
    let text_id = sqlite_string_literal(text_id);
    let title = sqlite_string_literal(title);
    contents.push_str(&format!(
        "INSERT INTO ideas_data(text_id, title, description, status, created_at, source) VALUES('{text_id}', '{title}', NULL, 'new', '2026-06-03T00:00:00Z', 'user');\n"
    ));
    std::fs::write(path, contents).expect("write ideas.sql");
}

#[cfg(unix)]
fn fake_gh_path(temp: &Path, script: &str) -> std::path::PathBuf {
    let bin = temp.join("bin");
    std::fs::create_dir_all(&bin).expect("create fake gh bin");
    let gh = bin.join("gh");
    std::fs::write(&gh, script).expect("write fake gh");
    let mut permissions = std::fs::metadata(&gh)
        .expect("fake gh metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&gh, permissions).expect("chmod fake gh");
    bin
}

#[cfg(unix)]
fn github_profile_fixture_path(owner: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/github_profile")
        .join(owner)
        .join(".exosuit/sidecars.toml")
}

#[test]
fn sidecar_discover_reports_registry_file_exact_match() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        r#"version = 1

[defaults]
root = "~/.exo/sidecars"
auto_push = "if_remote"

[projects."github.com/wycats/locald"]
key = "locald"
remote = "git@github.com:wycats/locald-exosuit-state.git"
"#,
    )
    .expect("write registry");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.discovery");
    assert_eq!(result["ok"], true);
    assert_eq!(result["repository"]["host"], "github.com");
    assert_eq!(result["repository"]["owner"], "wycats");
    assert_eq!(result["repository"]["repo"], "locald");
    assert_eq!(
        result["repository"]["remote"],
        "git@github.com:wycats/locald.git"
    );
    assert_eq!(result["identity"]["source"], "remote-owner-unknown");
    assert_eq!(result["identity"]["login"], "wycats");
    assert_eq!(result["registry"]["source"], "local-file");
    assert_eq!(
        result["registry"]["label"],
        format!(
            "local-file:{}",
            registry.to_str().expect("registry path is utf-8")
        )
    );
    assert_eq!(
        result["registry"]["path"],
        registry.to_str().expect("registry path is utf-8")
    );
    assert_eq!(result["registry"]["version"], 1);
    assert_eq!(result["attempt_index"], 0);
    assert_eq!(
        result["checked"].as_array().expect("checked array").len(),
        1
    );
    assert_eq!(result["checked"][0]["attempt_index"], 0);
    assert_eq!(result["checked"][0]["source"], "local-file");
    assert_eq!(
        result["checked"][0]["label"],
        format!(
            "local-file:{}",
            registry.to_str().expect("registry path is utf-8")
        )
    );
    assert_eq!(
        result["checked"][0]["path"],
        registry.to_str().expect("registry path is utf-8")
    );
    assert_eq!(result["checked"][0]["status"], "loaded-match");
    assert_eq!(result["match"]["kind"], "exact-project");
    assert_eq!(result["match"]["key"], "github.com/wycats/locald");
    assert_eq!(result["confidence"], "high");
    assert_eq!(
        result["source_summary"],
        format!(
            "local-file:{} matched github.com/wycats/locald with high confidence",
            registry.to_str().expect("registry path is utf-8")
        )
    );
    assert_eq!(result["proposal"]["key"], "locald");
    assert_eq!(result["proposal"]["root"], "~/.exo/sidecars");
    assert_eq!(
        result["proposal"]["remote"],
        "git@github.com:wycats/locald-exosuit-state.git"
    );
    assert_eq!(result["proposal"]["auto_push"], "if_remote");
    assert_eq!(result["proposal"]["would_mutate_config"], true);
    assert_eq!(result["proposal"]["requires_remote_acceptance"], false);
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions
            .iter()
            .any(|action| action["command"] == "exo sidecar bootstrap --discover")
    }));

    let human_output = exo_cmd(&repo, &home, &config_home)
        .args([
            "sidecar",
            "discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(human_output).expect("human output is utf-8");
    assert!(human.contains("Registry: local-file:"), "{human}");
    assert!(
        human.contains("Source: remote-owner-unknown wycats"),
        "{human}"
    );
    assert!(human.contains("Confidence: high"), "{human}");
    assert!(human.contains("Root: ~/.exo/sidecars"), "{human}");
    assert!(human.contains("Auto-push: if_remote"), "{human}");
    assert!(human.contains("Would mutate config: true"), "{human}");
    assert!(
        human.contains("Requires remote acceptance: false"),
        "{human}"
    );
    assert!(
        human.contains("Next actions:\n  → exo sidecar bootstrap --discover"),
        "{human}"
    );
}

#[test]
fn sidecar_discover_reports_no_github_remote() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "discover"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.discovery");
    assert_eq!(result["ok"], false);
    assert_eq!(result["failure"]["classification"], "no-github-remote");
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions
            .iter()
            .any(|action| action["command"] == "git remote add origin <github-url>")
    }));
    assert_no_work_repo_daemon_runtime(&repo);
}

#[test]
fn sidecar_discover_reports_git_required_for_non_git_directory() {
    let temp = short_tempdir();
    let repo = temp.path().join("visible-browser-lab");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "discover"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.discovery");
    assert_eq!(result["ok"], false);
    assert_eq!(result["requires_git_repo"], true);
    assert_eq!(result["failure"]["classification"], "git-required");
    assert!(
        result["failure"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("git init")),
        "{result:?}"
    );
    assert!(
        result["next_actions"].as_array().is_some_and(|actions| {
            actions.iter().any(|action| action["command"] == "git init")
        })
    );
    assert!(!config_home.join("exo/projects.toml").exists());
    assert_no_work_repo_daemon_runtime(&repo);

    let human_output = exo_cmd(&repo, &home, &config_home)
        .args(["sidecar", "discover"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(human_output).expect("human output is utf-8");
    assert!(human.contains("git-required"), "{human}");
    assert!(human.contains("git init"), "{human}");
}

#[test]
fn sidecar_bootstrap_reports_git_required_for_non_git_directory() {
    let temp = short_tempdir();
    let repo = temp.path().join("visible-browser-lab");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--key",
            "visible-browser-lab",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let envelope: JsonValue = serde_json::from_slice(&output).expect("valid json envelope");
    assert!(
        envelope["steering"].as_array().is_some_and(|actions| {
            actions.iter().any(|action| action["command"] == "git init")
        }),
        "{envelope:?}"
    );
    let error = json_error(&output);

    assert_eq!(error["code"], "precondition_failed");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("git init")),
        "{error:?}"
    );
    assert_eq!(error["details"]["requires_git_repo"], true);
    assert_eq!(error["details"]["next_command"], "git init");
    assert_eq!(
        error["details"]["default_sidecar_root"].as_str(),
        Some(
            home.join("exo/sidecars")
                .to_str()
                .expect("default root is utf-8")
        )
    );
    assert!(!config_home.join("exo/projects.toml").exists());
    assert_no_work_repo_daemon_runtime(&repo);
}

#[test]
fn sidecar_bootstrap_discover_reports_git_required_as_discovery_payload() {
    let temp = short_tempdir();
    let repo = temp.path().join("visible-browser-lab");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap", "--discover"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.discovery");
    assert_eq!(result["ok"], false);
    assert_eq!(result["requires_git_repo"], true);
    assert_eq!(result["failure"]["classification"], "git-required");
    assert!(
        result["next_actions"].as_array().is_some_and(|actions| {
            actions.iter().any(|action| action["command"] == "git init")
        })
    );
    assert!(!config_home.join("exo/projects.toml").exists());
    assert_no_work_repo_daemon_runtime(&repo);
}

#[test]
fn sidecar_bootstrap_discover_failure_is_structured_without_work_repo_daemon_runtime() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap", "--discover"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.discovery");
    assert_eq!(result["ok"], false);
    assert_eq!(result["requires_git_repo"], false);
    assert_eq!(result["failure"]["classification"], "no-github-remote");
    assert_no_work_repo_daemon_runtime(&repo);
}

fn assert_sidecar_discover_failure(registry_contents: &str, classification: &str) -> JsonValue {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(&registry, registry_contents).expect("write registry");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.discovery");
    assert_eq!(result["ok"], false);
    assert_eq!(result["failure"]["classification"], classification);
    assert_eq!(result["attempt_index"], 0);
    assert_eq!(
        result["checked"].as_array().expect("checked array").len(),
        1
    );
    assert_eq!(result["checked"][0]["attempt_index"], 0);
    assert_eq!(result["checked"][0]["source"], "local-file");
    assert_eq!(
        result["checked"][0]["path"],
        registry.to_str().expect("registry path is utf-8")
    );
    let expected_status = match classification {
        "registry-parse-error" => "parse-error",
        "unsafe-registry-value" => "unsafe-value",
        "no-matching-entry" => "loaded-no-match",
        other => panic!("unexpected classification {other}"),
    };
    assert_eq!(result["checked"][0]["status"], expected_status);
    assert_eq!(
        result["failure"]["source"],
        registry.to_str().expect("registry path is utf-8")
    );
    assert!(
        result["failure"]["message"]
            .as_str()
            .is_some_and(|message| !message.is_empty())
    );
    assert!(
        result["source_summary"].as_str().is_some_and(|summary| {
            summary.contains("did not produce a usable sidecar discovery")
        })
    );
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions.iter().any(|action| {
            action["command"].as_str().is_some_and(|command| {
                command.starts_with("exo sidecar discover --registry-file ")
                    && command.ends_with(" --verbose")
                    && command.contains(registry.to_str().expect("registry path is utf-8"))
            })
        })
    }));

    result
}

#[cfg(unix)]
#[test]
fn sidecar_discover_fetches_authenticated_user_profile_registry() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let profile_fixture = github_profile_fixture_path("wycats");
    let fake_path = fake_gh_path(
        temp.path(),
        &format!(
            r#"#!/bin/sh
set -eu
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "user" ] && [ "$3" = "--jq" ] && [ "$4" = ".login" ]; then
    echo "wycats"
    exit 0
fi
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "-H" ] && [ "$4" = "repos/wycats/wycats/contents/.exosuit/sidecars.toml" ]; then
    cat {fixture}
    exit 0
fi
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "users/wycats" ] && [ "$3" = "--jq" ] && [ "$4" = ".type" ]; then
    echo "User"
    exit 0
fi
echo "unexpected gh invocation: $*" >&2
exit 2
"#,
            fixture = profile_fixture.to_str().expect("fixture path is utf-8")
        ),
    );
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "discover"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                fake_path.to_str().expect("fake PATH is utf-8"),
                std::env::var("PATH").expect("PATH is set")
            ),
        )
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.discovery");
    assert_eq!(result["ok"], true);
    assert_eq!(result["identity"]["source"], "authenticated-user");
    assert_eq!(result["identity"]["login"], "wycats");
    assert_eq!(result["registry"]["source"], "github-profile");
    assert_eq!(
        result["registry"]["label"],
        "github-profile:.exosuit/sidecars.toml"
    );
    assert_eq!(
        result["registry"]["profile_repo"],
        "github.com/wycats/wycats"
    );
    assert_eq!(result["registry"]["path"], ".exosuit/sidecars.toml");
    assert_eq!(result["registry"]["version"], 1);
    assert_eq!(result["attempt_index"], 0);
    assert_eq!(
        result["checked"].as_array().expect("checked array").len(),
        3
    );
    assert_eq!(result["checked"][0]["attempt_index"], 0);
    assert_eq!(result["checked"][0]["source"], "github-profile");
    assert_eq!(
        result["checked"][0]["identity_source"],
        "authenticated-user"
    );
    assert_eq!(result["checked"][0]["identity_login"], "wycats");
    assert_eq!(
        result["checked"][0]["profile_repo"],
        "github.com/wycats/wycats"
    );
    assert_eq!(result["checked"][0]["path"], ".exosuit/sidecars.toml");
    assert_eq!(result["checked"][0]["status"], "loaded-match");
    assert_eq!(result["checked"][1]["status"], "skipped");
    assert_eq!(result["checked"][2]["status"], "fetched");
    assert_eq!(result["match"]["kind"], "exact-project");
    assert_eq!(result["match"]["key"], "github.com/wycats/locald");
    assert_eq!(result["confidence"], "high");
    assert_eq!(result["proposal"]["key"], "locald");
    assert_eq!(result["proposal"]["root"], "~/.exo/sidecars");
    assert_eq!(
        result["proposal"]["remote"],
        "git@github.com:wycats/locald-exosuit-state.git"
    );
    assert_eq!(result["proposal"]["auto_push"], "if_remote");
    assert_eq!(result["proposal"]["would_mutate_config"], true);
    assert_eq!(result["proposal"]["requires_remote_acceptance"], false);
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions
            .iter()
            .any(|action| action["command"] == "exo sidecar bootstrap --discover")
    }));

    let human_output = exo_cmd(&repo, &home, &config_home)
        .args(["sidecar", "discover"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                fake_path.to_str().expect("fake PATH is utf-8"),
                std::env::var("PATH").expect("PATH is set")
            ),
        )
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(human_output).expect("human output is utf-8");
    assert!(
        human.contains("Registry location: github.com/wycats/wycats:.exosuit/sidecars.toml"),
        "{human}"
    );
}

#[cfg(unix)]
#[test]
fn sidecar_discover_fetches_remote_owner_organization_profile_registry() {
    let temp = short_tempdir();
    let repo = temp.path().join("widget");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let fake_path = fake_gh_path(
        temp.path(),
        r#"#!/bin/sh
set -eu
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "user" ] && [ "$3" = "--jq" ] && [ "$4" = ".login" ]; then
    echo "not logged in" >&2
    exit 1
fi
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "users/acme" ] && [ "$3" = "--jq" ] && [ "$4" = ".type" ]; then
    echo "Organization"
    exit 0
fi
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "-H" ] && [ "$4" = "repos/acme/.github/contents/.exosuit/sidecars.toml" ]; then
    cat <<'EOF'
version = 1

[defaults]
root = "~/.exo/sidecars"
auto_push = "if_remote"

[projects."github.com/acme/widget"]
key = "widget"
remote = "git@github.com:acme/widget-exosuit-state.git"
EOF
    exit 0
fi
echo "unexpected gh invocation: $*" >&2
exit 2
"#,
    );
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &["remote", "add", "origin", "git@github.com:acme/widget.git"],
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "discover"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                fake_path.to_str().expect("fake PATH is utf-8"),
                std::env::var("PATH").expect("PATH is set")
            ),
        )
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.discovery");
    assert_eq!(result["ok"], true);
    assert_eq!(result["repository"]["owner"], "acme");
    assert_eq!(result["repository"]["repo"], "widget");
    assert_eq!(result["identity"]["source"], "remote-owner-organization");
    assert_eq!(result["identity"]["login"], "acme");
    assert_eq!(result["registry"]["source"], "github-organization-profile");
    assert_eq!(
        result["registry"]["label"],
        "github-organization-profile:.exosuit/sidecars.toml"
    );
    assert_eq!(
        result["registry"]["profile_repo"],
        "github.com/acme/.github"
    );
    assert_eq!(result["registry"]["path"], ".exosuit/sidecars.toml");
    assert_eq!(result["registry"]["version"], 1);
    assert_eq!(result["attempt_index"], 1);
    assert_eq!(
        result["checked"].as_array().expect("checked array").len(),
        3
    );
    assert_eq!(result["checked"][0]["attempt_index"], 0);
    assert_eq!(result["checked"][0]["source"], "github-profile");
    assert_eq!(
        result["checked"][0]["identity_source"],
        "authenticated-user"
    );
    assert_eq!(result["checked"][0]["status"], "skipped");
    assert_eq!(result["checked"][1]["attempt_index"], 1);
    assert_eq!(
        result["checked"][1]["source"],
        "github-organization-profile"
    );
    assert_eq!(
        result["checked"][1]["identity_source"],
        "remote-owner-organization"
    );
    assert_eq!(result["checked"][1]["identity_login"], "acme");
    assert_eq!(
        result["checked"][1]["profile_repo"],
        "github.com/acme/.github"
    );
    assert_eq!(result["checked"][1]["path"], ".exosuit/sidecars.toml");
    assert_eq!(result["checked"][1]["status"], "loaded-match");
    assert_eq!(result["checked"][2]["attempt_index"], 2);
    assert_eq!(result["checked"][2]["source"], "github-profile");
    assert_eq!(
        result["checked"][2]["identity_source"],
        "remote-owner-organization"
    );
    assert_eq!(result["checked"][2]["status"], "skipped");
    assert_eq!(result["match"]["kind"], "exact-project");
    assert_eq!(result["match"]["key"], "github.com/acme/widget");
    assert_eq!(result["confidence"], "high");
    assert_eq!(result["proposal"]["key"], "widget");
    assert_eq!(result["proposal"]["root"], "~/.exo/sidecars");
    assert_eq!(
        result["proposal"]["remote"],
        "git@github.com:acme/widget-exosuit-state.git"
    );
    assert_eq!(result["proposal"]["auto_push"], "if_remote");
    assert_eq!(result["proposal"]["would_mutate_config"], true);
    assert_eq!(result["proposal"]["requires_remote_acceptance"], false);
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions
            .iter()
            .any(|action| action["command"] == "exo sidecar bootstrap --discover")
    }));
}

#[cfg(unix)]
#[test]
fn sidecar_discover_reports_registry_not_found_for_missing_registry_source() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let fake_path = fake_gh_path(
        temp.path(),
        r#"#!/bin/sh
set -eu
if [ "$1" = "api" ] && [ "$2" = "user" ]; then
    echo "not logged in" >&2
    exit 1
fi
if [ "$1" = "api" ] && [ "$2" = "users/wycats" ]; then
    echo "Unknown"
    exit 0
fi
if [ "$1" = "api" ] && [ "$2" = "-H" ] && [ "$4" = "repos/wycats/.github/contents/.exosuit/sidecars.toml" ]; then
    echo "HTTP 404: Not Found" >&2
    exit 1
fi
if [ "$1" = "api" ] && [ "$2" = "-H" ] && [ "$4" = "repos/wycats/wycats/contents/.exosuit/sidecars.toml" ]; then
    echo "HTTP 404: Not Found" >&2
    exit 1
fi
echo "unexpected gh invocation: $*" >&2
exit 2
"#,
    );
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "discover"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                fake_path.to_str().expect("fake PATH is utf-8"),
                std::env::var("PATH").expect("PATH is set")
            ),
        )
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["ok"], false);
    assert_eq!(result["failure"]["classification"], "registry-not-found");
    assert!(result.get("attempt_index").is_none());
    assert_eq!(
        result["checked"].as_array().expect("checked array").len(),
        3
    );
    assert_eq!(result["checked"][0]["attempt_index"], 0);
    assert_eq!(result["checked"][0]["source"], "github-profile");
    assert_eq!(
        result["checked"][0]["identity_source"],
        "authenticated-user"
    );
    assert_eq!(result["checked"][0]["status"], "skipped");
    assert_eq!(
        result["checked"][1]["source"],
        "github-organization-profile"
    );
    assert_eq!(
        result["checked"][1]["identity_source"],
        "remote-owner-unknown"
    );
    assert_eq!(result["checked"][1]["status"], "not-found");
    assert_eq!(result["checked"][0]["path"], ".exosuit/sidecars.toml");
    assert_eq!(result["checked"][2]["source"], "github-profile");
    assert_eq!(
        result["checked"][2]["identity_source"],
        "remote-owner-unknown"
    );
    assert_eq!(result["checked"][2]["status"], "not-found");
    assert_eq!(
        result["checked"][2]["profile_repo"],
        "github.com/wycats/wycats"
    );
    assert!(
        result["failure"]["source"]
            .as_str()
            .is_some_and(|source| { source == "github.com/wycats/wycats:.exosuit/sidecars.toml" })
    );
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions
            .iter()
            .any(|action| action["command"] == "exo sidecar discover --verbose")
    }));

    let human_output = exo_cmd(&repo, &home, &config_home)
        .args(["sidecar", "discover"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                fake_path.to_str().expect("fake PATH is utf-8"),
                std::env::var("PATH").expect("PATH is set")
            ),
        )
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(human_output).expect("human output is utf-8");
    assert!(
        human.contains("Registry: github-profile:.exosuit/sidecars.toml"),
        "{human}"
    );
    assert!(human.contains("Failure: registry-not-found"), "{human}");
    assert!(human.contains("Next actions:"), "{human}");
    assert!(
        human.contains("→ exo sidecar discover --verbose"),
        "{human}"
    );
}

#[test]
fn sidecar_discover_reports_registry_parse_error() {
    assert_sidecar_discover_failure("version =", "registry-parse-error");
    assert_sidecar_discover_failure("version = 2", "registry-parse-error");
}

#[test]
fn sidecar_discover_reports_unsafe_registry_value() {
    assert_sidecar_discover_failure(
        r#"version = 1

[defaults]
remote_template = "https://gitlab.com/{owner}/{repo}.git"
"#,
        "unsafe-registry-value",
    );
}

#[test]
fn sidecar_discover_reports_no_matching_entry() {
    let result = assert_sidecar_discover_failure(
        r#"version = 1

[projects."github.com/wycats/other"]
key = "other"
remote = "git@github.com:wycats/other-state.git"
"#,
        "no-matching-entry",
    );

    assert_eq!(result["match"]["kind"], "none");
    assert_eq!(result["confidence"], "none");
}

#[test]
fn ordinary_update_uses_daemon_writer_lane_for_sidecar_state() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let diagnostics_path = temp.path().join("sidecar-update-daemon.ndjson");
    std::fs::create_dir_all(repo.join("docs/agent-context")).expect("create agent-context dir");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    std::fs::write(sidecar_root.join("README.md"), "sidecar\n").expect("write readme");
    git_success(&sidecar_root, &["add", "README.md"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial sidecar"]);
    std::fs::write(
        repo.join("docs/agent-context/plan.toml"),
        r#"[[epochs]]
id = "legacy-epoch"
title = "Legacy Epoch"
status = "active"

[[epochs.phases]]
id = "legacy-phase"
title = "Legacy Phase"
status = "active"
tasks = ["Legacy Goal"]
"#,
    )
    .expect("write legacy plan");
    std::fs::write(
        repo.join("docs/agent-context/axioms.sql"),
        "-- legacy axiom dump\n",
    )
    .expect("write legacy axiom dump");
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    let _guard = DaemonPathGuard::new(&repo);

    exo_cmd(&repo, &home, &config_home)
        .env("EXO_DAEMON_DIAGNOSTICS", "1")
        .env("EXO_DAEMON_DIAG_PATH", &diagnostics_path)
        .args(["--format", "json", "idea", "add", "Acquire Sidecar Writer"])
        .assert()
        .success();
    assert!(
        sidecar_write_owner_marker_path(&sidecar_root, "external-test").exists(),
        "normal daemon write should acquire sidecar writer ownership"
    );
    let owner: JsonValue = serde_json::from_str(
        &std::fs::read_to_string(sidecar_write_owner_marker_path(
            &sidecar_root,
            "external-test",
        ))
        .expect("read sidecar writer ownership"),
    )
    .expect("sidecar writer ownership is json");
    let daemon_pid = std::fs::read_to_string(project_state_path(
        &sidecar_root,
        "external-test",
        &["runtime", "daemon.pid"],
    ))
    .expect("read sidecar daemon pid");
    assert_eq!(
        owner["pid"].as_u64(),
        daemon_pid.trim().parse::<u64>().ok(),
        "normal write should establish ownership through the sidecar daemon writer lane"
    );

    let update_output = exo_cmd(&repo, &home, &config_home)
        .env("EXO_DAEMON_DIAGNOSTICS", "1")
        .env("EXO_DAEMON_DIAG_PATH", &diagnostics_path)
        .args(["--format", "json", "update"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let update = json_result(&update_output);
    let events = wait_for_daemon_operation(&diagnostics_path, "", "update");
    assert_has_daemon_operation(&events, "", "update");
    assert_eq!(
        update["post_write"]["sidecar_auto_persist"]["ok"], true,
        "ordinary update should complete its sidecar checkpoint: {update:#}"
    );
    assert_eq!(
        update["post_write"]["sidecar_auto_persist"]["committed"], true,
        "ordinary update should commit its sidecar checkpoint: {update:#}"
    );
    assert_no_work_repo_daemon_runtime(&repo);
    let applied = update["applied"].as_array().expect("applied is array");
    assert!(
        applied
            .iter()
            .any(|report| report["plugin_id"] == "migrate-legacy-plan-v1"),
        "expected legacy plan migration in applied reports: {update:#}"
    );

    let epoch_output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "epoch", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let epoch_list = json_result(&epoch_output);
    let epochs = epoch_list["epochs"].as_array().expect("epochs is array");
    assert_eq!(epochs.len(), 1);
    assert_eq!(epochs[0]["title"], "Legacy Epoch");

    let sidecar_epochs = std::fs::read_to_string(
        sidecar_root.join("projects/external-test/agent-context/epochs.sql"),
    )
    .expect("read sidecar epoch projection");
    assert!(sidecar_epochs.contains("Legacy Epoch"));
}

fn disable_sidecar_auto_commit(config_home: &Path) {
    let path = config_home.join("exo/projects.toml");
    let mut policy = std::fs::read_to_string(&path).expect("read project policy");
    if policy.contains("auto_commit") {
        return;
    }
    policy = policy
        .lines()
        .map(|line| {
            if line.contains("state = \"sidecar\"") && line.contains('}') {
                line.replacen(" }", ", auto_commit = false }", 1)
            } else if line.trim() == "state = \"sidecar\"" {
                "state = \"sidecar\"\nauto_commit = false".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    policy.push('\n');
    std::fs::write(path, policy).expect("write project policy");
}

fn set_sidecar_auto_push(config_home: &Path, policy_value: &str) {
    let path = config_home.join("exo/projects.toml");
    let mut policy = std::fs::read_to_string(&path).expect("read project policy");
    policy = policy.replace(
        "state = \"sidecar\"\n",
        &format!("state = \"sidecar\"\nauto_push = {policy_value:?}\n"),
    );
    std::fs::write(path, policy).expect("write project policy");
}

#[test]
fn sidecar_link_bootstraps_without_repo_metadata() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    let config_path = config_home.join("exo/projects.toml");
    std::fs::create_dir_all(config_path.parent().expect("config parent")).expect("create config");
    std::fs::write(
        &config_path,
        "# preserve\n[projects.other]\nstate = \"shadow\"\n",
    )
    .expect("write existing config");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "link",
            "--key",
            "external-test",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.link");
    assert_eq!(result["sidecar_key"], "external-test");
    assert!(project_state_path(&sidecar_root, "external-test", &["sidecar.toml"]).exists());
    assert!(project_state_path(&sidecar_root, "external-test", &["agent-context"]).exists());
    assert!(project_state_path(&sidecar_root, "external-test", &["cache", "exo.db"]).exists());
    assert!(!project_state_path(&sidecar_root, "external-test", &["runtime"]).exists());
    assert!(!home.join(".exo/sidecars/external-test").exists());
    assert!(!repo.join(".exo").exists());
    assert!(!repo.join("exosuit.toml").exists());

    let policy = std::fs::read_to_string(&config_path).expect("read policy");
    assert!(policy.contains("# preserve"));
    assert!(policy.contains("[projects.other]"));
    assert!(policy.contains("state = \"sidecar\""));
    assert!(policy.contains("sidecar_key = \"external-test\""));

    let resolve_output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "project", "resolve"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let resolve = json_result(&resolve_output);
    assert_eq!(resolve["project"]["policy"], "sidecar");
    assert_eq!(resolve["project"]["sidecar_key"], "external-test");
    assert_eq!(
        resolve["paths"]["db_path"].as_str(),
        Some(
            project_state_path(&sidecar_root, "external-test", &["cache", "exo.db"])
                .to_str()
                .expect("db path is utf-8")
        )
    );
    assert_eq!(
        resolve["paths"]["state_root"].as_str(),
        Some(
            project_state_root(&sidecar_root, "external-test")
                .to_str()
                .expect("state root is utf-8")
        )
    );
    assert_eq!(
        resolve["paths"]["sidecar_projection_dir"].as_str(),
        Some(
            project_state_path(&sidecar_root, "external-test", &["agent-context"])
                .to_str()
                .expect("projection path is utf-8")
        )
    );
}

#[test]
fn project_move_root_dry_run_reports_changes_without_writing() {
    let temp = short_tempdir();
    let old_repo = temp.path().join("old-repo");
    let new_repo = temp.path().join("new-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&old_repo).expect("create old repo");
    std::fs::create_dir_all(&new_repo).expect("create new repo");
    git_init(&old_repo);
    git_init(&new_repo);
    link_sidecar_with_key(
        &old_repo,
        &home,
        &config_home,
        &sidecar_root,
        "move-root-test",
    );
    let old_project_id = project_id_for(&old_repo, &home, &config_home);
    let new_project_id = project_id_for(&new_repo, &home, &config_home);
    let canonical_new_repo = new_repo.canonicalize().expect("canonical new repo");
    seed_sidecar_workspace_root_state(&sidecar_root, "move-root-test", &old_repo);
    let old_rfc_path = create_rfc_00001(&old_repo, &home, &config_home);
    copy_rfc_file_to_new_root(&old_rfc_path, &old_repo, &new_repo);

    let output = exo_direct_cmd(&old_repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "project",
            "move-root",
            "--key",
            "move-root-test",
            "--to",
            new_repo.to_str().expect("new repo path is utf-8"),
            "--dry-run",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "project.move_root");
    assert_eq!(result["dry_run"], true);
    assert_eq!(result["apply_ready"], true);
    assert_eq!(result["old_policy_project_id"], old_project_id);
    assert_eq!(result["new_policy_project_id"], new_project_id);
    assert_eq!(
        result["old_workspace_root"].as_str(),
        Some(old_repo.to_string_lossy().as_ref())
    );
    assert_eq!(
        result["new_workspace_root"].as_str(),
        Some(canonical_new_repo.to_string_lossy().as_ref())
    );
    assert_eq!(result["verification"]["rfc_00001_found"], true);
    let rfc_00001_path = PathBuf::from(
        result["verification"]["rfc_00001_path"]
            .as_str()
            .expect("RFC 00001 path"),
    );
    assert!(
        rfc_00001_path.exists(),
        "reported RFC path should exist: {}",
        rfc_00001_path.display()
    );
    assert!(
        rfc_00001_path
            .to_string_lossy()
            .contains("docs/rfcs/stage-0/00001-"),
        "reported RFC path should include the stage directory: {}",
        rfc_00001_path.display()
    );
    assert!(policy_contains_project_id(&config_home, &old_project_id));
    assert!(!policy_contains_project_id(&config_home, &new_project_id));
    assert_eq!(
        sidecar_workspace_roots(
            &sidecar_root,
            "move-root-test",
            "workspace_active_phase_data",
            "workspace_root"
        ),
        vec![old_repo.to_string_lossy().to_string()]
    );
}

#[test]
fn project_move_root_retargets_policy_state_and_preserves_rfc_lookup() {
    let temp = short_tempdir();
    let old_repo = temp.path().join("old-repo");
    let new_repo = temp.path().join("new-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&old_repo).expect("create old repo");
    std::fs::create_dir_all(&new_repo).expect("create new repo");
    git_init(&old_repo);
    git_init(&new_repo);
    link_sidecar_with_key(
        &old_repo,
        &home,
        &config_home,
        &sidecar_root,
        "move-root-test",
    );
    let old_project_id = project_id_for(&old_repo, &home, &config_home);
    let new_project_id = project_id_for(&new_repo, &home, &config_home);
    let canonical_new_repo = new_repo.canonicalize().expect("canonical new repo");
    seed_sidecar_workspace_root_state(&sidecar_root, "move-root-test", &old_repo);
    let old_rfc_path = create_rfc_00001(&old_repo, &home, &config_home);
    copy_rfc_file_to_new_root(&old_rfc_path, &old_repo, &new_repo);

    let output = exo_direct_cmd(&old_repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "project",
            "move-root",
            "--key",
            "move-root-test",
            "--to",
            new_repo.to_str().expect("new repo path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["dry_run"], false);
    assert_eq!(result["apply_ready"], true);
    assert_eq!(result["sidecar_project_id"], new_project_id);
    assert!(!policy_contains_project_id(&config_home, &old_project_id));
    assert!(policy_contains_project_id(&config_home, &new_project_id));
    assert_eq!(
        sidecar_manifest_project_id(&sidecar_root, "move-root-test"),
        new_project_id
    );
    assert_eq!(
        policy_value_for_sidecar_key(&config_home, "move-root-test", "sidecar_root"),
        sidecar_root.to_string_lossy()
    );
    assert_eq!(
        sidecar_workspace_roots(
            &sidecar_root,
            "move-root-test",
            "workspace_active_phase_data",
            "workspace_root"
        ),
        vec![canonical_new_repo.to_string_lossy().to_string()]
    );
    assert_eq!(
        sidecar_workspace_roots(
            &sidecar_root,
            "move-root-test",
            "phase_ownership_data",
            "claimed_by_workspace_root"
        ),
        vec![canonical_new_repo.to_string_lossy().to_string()]
    );
    let expected_workspace_id = test_workspace_id(&new_project_id, &canonical_new_repo);
    assert_eq!(
        phase_owner_workspace_ids(&sidecar_root, "move-root-test", &canonical_new_repo),
        (expected_workspace_id.clone(), expected_workspace_id)
    );

    let show_output = exo_direct_cmd(&new_repo, &home, &config_home)
        .args(["--format", "json", "rfc", "show", "00001"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let show = json_result(&show_output);
    assert_eq!(show["id"], "00001");
    assert_eq!(show["title"], "Root Move RFC");
}

#[test]
fn project_move_root_collapses_identical_destination_policy_binding() {
    let temp = short_tempdir();
    let old_repo = temp.path().join("old-repo");
    let new_repo = temp.path().join("new-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&old_repo).expect("create old repo");
    std::fs::create_dir_all(&new_repo).expect("create new repo");
    git_init(&old_repo);
    git_init(&new_repo);
    link_sidecar_with_key(
        &old_repo,
        &home,
        &config_home,
        &sidecar_root,
        "move-root-test",
    );
    let old_project_id = project_id_for(&old_repo, &home, &config_home);
    let new_project_id = project_id_for(&new_repo, &home, &config_home);
    let canonical_new_repo = new_repo.canonicalize().expect("canonical new repo");
    seed_sidecar_workspace_root_state(&sidecar_root, "move-root-test", &old_repo);
    let old_rfc_path = create_rfc_00001(&old_repo, &home, &config_home);
    copy_rfc_file_to_new_root(&old_rfc_path, &old_repo, &new_repo);

    let policy_path = config_home.join("exo").join("projects.toml");
    let mut policy = std::fs::read_to_string(&policy_path).expect("read project policy");
    policy.push_str(&format!(
        "\n[projects.\"{new_project_id}\"]\nstate = \"sidecar\"\nsidecar_key = \"move-root-test\"\nsidecar_root = \"{}\"\n",
        sidecar_root.to_string_lossy()
    ));
    std::fs::write(&policy_path, policy).expect("write duplicate project policy");

    let output = exo_direct_cmd(&old_repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "project",
            "move-root",
            "--key",
            "move-root-test",
            "--to",
            new_repo.to_str().expect("new repo path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["apply_ready"], true);
    assert!(!policy_contains_project_id(&config_home, &old_project_id));
    assert!(policy_contains_project_id(&config_home, &new_project_id));
    assert_eq!(
        sidecar_manifest_project_id(&sidecar_root, "move-root-test"),
        new_project_id
    );
    assert_eq!(
        sidecar_workspace_roots(
            &sidecar_root,
            "move-root-test",
            "workspace_active_phase_data",
            "workspace_root"
        ),
        vec![canonical_new_repo.to_string_lossy().to_string()]
    );
}

#[test]
fn project_move_root_rejects_destination_policy_option_mismatch() {
    let temp = short_tempdir();
    let old_repo = temp.path().join("old-repo");
    let new_repo = temp.path().join("new-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&old_repo).expect("create old repo");
    std::fs::create_dir_all(&new_repo).expect("create new repo");
    git_init(&old_repo);
    git_init(&new_repo);
    link_sidecar_with_key(
        &old_repo,
        &home,
        &config_home,
        &sidecar_root,
        "move-root-test",
    );
    let old_project_id = project_id_for(&old_repo, &home, &config_home);
    let new_project_id = project_id_for(&new_repo, &home, &config_home);
    seed_sidecar_workspace_root_state(&sidecar_root, "move-root-test", &old_repo);

    let policy_path = config_home.join("exo").join("projects.toml");
    let mut policy = std::fs::read_to_string(&policy_path).expect("read project policy");
    policy.push_str(&format!(
        "\n[projects.\"{new_project_id}\"]\nstate = \"sidecar\"\nsidecar_key = \"move-root-test\"\nsidecar_root = \"{}\"\nauto_push = \"never\"\n",
        sidecar_root.to_string_lossy()
    ));
    std::fs::write(&policy_path, policy).expect("write duplicate project policy");

    let output = exo_direct_cmd(&old_repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "project",
            "move-root",
            "--key",
            "move-root-test",
            "--to",
            new_repo.to_str().expect("new repo path is utf-8"),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    let message = error["message"].as_str().expect("error message");
    assert!(
        message.contains("already has a different local project policy entry"),
        "{message}"
    );
    assert!(policy_contains_project_id(&config_home, &old_project_id));
    assert!(policy_contains_project_id(&config_home, &new_project_id));
}

#[test]
fn project_move_root_runs_from_destination_checkout_with_relative_to() {
    let temp = short_tempdir();
    let old_repo = temp.path().join("old-repo");
    let new_repo = temp.path().join("new-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&old_repo).expect("create old repo");
    std::fs::create_dir_all(&new_repo).expect("create new repo");
    git_init(&old_repo);
    git_init(&new_repo);
    link_sidecar_with_key(
        &old_repo,
        &home,
        &config_home,
        &sidecar_root,
        "move-root-test",
    );
    let old_project_id = project_id_for(&old_repo, &home, &config_home);
    let new_project_id = project_id_for(&new_repo, &home, &config_home);
    let canonical_new_repo = new_repo.canonicalize().expect("canonical new repo");
    seed_sidecar_workspace_root_state(&sidecar_root, "move-root-test", &old_repo);
    let old_rfc_path = create_rfc_00001(&old_repo, &home, &config_home);
    copy_rfc_file_to_new_root(&old_rfc_path, &old_repo, &new_repo);

    let output = exo_cmd(&new_repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "project",
            "move-root",
            "--key",
            "move-root-test",
            "--to",
            ".",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["apply_ready"], true);
    assert_eq!(
        result["new_workspace_root"].as_str(),
        Some(canonical_new_repo.to_string_lossy().as_ref())
    );
    assert!(!policy_contains_project_id(&config_home, &old_project_id));
    assert!(policy_contains_project_id(&config_home, &new_project_id));
    assert_eq!(
        sidecar_manifest_project_id(&sidecar_root, "move-root-test"),
        new_project_id
    );
}

#[test]
fn project_move_root_blocks_unknown_write_owner_marker_for_old_root() {
    let temp = short_tempdir();
    let old_repo = temp.path().join("old-repo");
    let new_repo = temp.path().join("new-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&old_repo).expect("create old repo");
    std::fs::create_dir_all(&new_repo).expect("create new repo");
    git_init(&old_repo);
    git_init(&new_repo);
    link_sidecar_with_key(
        &old_repo,
        &home,
        &config_home,
        &sidecar_root,
        "move-root-test",
    );
    seed_sidecar_workspace_root_state(&sidecar_root, "move-root-test", &old_repo);
    let marker_path = sidecar_write_owner_marker_path(&sidecar_root, "move-root-test");
    std::fs::create_dir_all(marker_path.parent().expect("marker parent"))
        .expect("create marker parent");
    std::fs::write(
        &marker_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "version": 1,
            "sidecar_key": "move-root-test",
            "sidecar_root": sidecar_root.to_string_lossy(),
            "workspace_root": old_repo.to_string_lossy(),
        }))
        .expect("serialize marker"),
    )
    .expect("write marker");

    let output = exo_direct_cmd(&old_repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "project",
            "move-root",
            "--key",
            "move-root-test",
            "--to",
            new_repo.to_str().expect("new repo path is utf-8"),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    let message = error["message"].as_str().expect("error message");
    assert!(message.contains("has unknown liveness"), "{message}");
}

#[cfg(unix)]
#[test]
fn project_move_root_blocks_active_destination_write_owner_marker() {
    let temp = short_tempdir();
    let old_repo = temp.path().join("old-repo");
    let new_repo = temp.path().join("new-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&old_repo).expect("create old repo");
    std::fs::create_dir_all(&new_repo).expect("create new repo");
    git_init(&old_repo);
    git_init(&new_repo);
    link_sidecar_with_key(
        &old_repo,
        &home,
        &config_home,
        &sidecar_root,
        "move-root-test",
    );
    seed_sidecar_workspace_root_state(&sidecar_root, "move-root-test", &old_repo);
    let canonical_new_repo = new_repo.canonicalize().expect("canonical new repo");

    let mut child = Command::new("sleep")
        .arg("60")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn live owner process");
    write_sidecar_write_owner_marker(
        &sidecar_root,
        "move-root-test",
        child.id(),
        &canonical_new_repo,
        None,
        None,
    );

    let output = exo_direct_cmd(&old_repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "project",
            "move-root",
            "--key",
            "move-root-test",
            "--to",
            new_repo.to_str().expect("new repo path is utf-8"),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let _ = child.kill();
    let _ = child.wait();
    let error = json_error(&output);

    let message = error["message"].as_str().expect("error message");
    assert!(message.contains("belongs to a live process"), "{message}");
}

#[test]
fn project_move_root_refuses_to_collapse_old_and_new_active_roots() {
    let temp = short_tempdir();
    let old_repo = temp.path().join("old-repo");
    let new_repo = temp.path().join("new-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&old_repo).expect("create old repo");
    std::fs::create_dir_all(&new_repo).expect("create new repo");
    git_init(&old_repo);
    git_init(&new_repo);
    link_sidecar_with_key(
        &old_repo,
        &home,
        &config_home,
        &sidecar_root,
        "move-root-test",
    );
    let canonical_new_repo = new_repo.canonicalize().expect("canonical new repo");
    seed_sidecar_workspace_root_state(&sidecar_root, "move-root-test", &old_repo);

    let db_path = project_state_path(&sidecar_root, "move-root-test", &["cache", "exo.db"]);
    let db = exosuit_storage::open_database(&db_path).expect("open sidecar db");
    db.connection()
        .execute(
            "INSERT INTO workspace_active_phase (workspace_root, phase_id, updated_at)
             SELECT ?1, id, '2026-06-22T00:00:01.000Z'
             FROM phases
             WHERE text_id = 'move-root-phase'",
            [canonical_new_repo.to_string_lossy().as_ref()],
        )
        .expect("insert new active root");

    let output = exo_direct_cmd(&old_repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "project",
            "move-root",
            "--key",
            "move-root-test",
            "--to",
            new_repo.to_str().expect("new repo path is utf-8"),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    let message = error["message"].as_str().expect("error message");
    assert!(
        message.contains("both old and new workspace roots have active project state"),
        "{message}"
    );
}

#[test]
fn project_move_root_absorbs_completed_source_active_root() {
    let temp = short_tempdir();
    let old_repo = temp.path().join("old-repo");
    let new_repo = temp.path().join("new-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&old_repo).expect("create old repo");
    std::fs::create_dir_all(&new_repo).expect("create new repo");
    git_init(&old_repo);
    git_init(&new_repo);
    link_sidecar_with_key(
        &old_repo,
        &home,
        &config_home,
        &sidecar_root,
        "move-root-test",
    );
    let new_project_id = project_id_for(&new_repo, &home, &config_home);
    let canonical_new_repo = new_repo.canonicalize().expect("canonical new repo");
    seed_sidecar_workspace_root_state(&sidecar_root, "move-root-test", &old_repo);

    let db_path = project_state_path(&sidecar_root, "move-root-test", &["cache", "exo.db"]);
    let db = exosuit_storage::open_database(&db_path).expect("open sidecar db");
    db.connection()
        .execute(
            "UPDATE phases
             SET status = 'completed'
             WHERE text_id = 'move-root-phase'",
            [],
        )
        .expect("mark source active phase completed");
    db.connection()
        .execute(
            "INSERT INTO phases (text_id, title, status, epoch_id, kind, slug, sort_key)
             SELECT 'move-root-current-phase', 'Move Root Current Phase', 'in-progress',
                    id, 'regular', NULL, '00000002'
             FROM epochs
             WHERE text_id = 'move-root-epoch'",
            [],
        )
        .expect("insert destination active phase");
    db.connection()
        .execute(
            "INSERT INTO workspace_active_phase (workspace_root, phase_id, updated_at)
             SELECT ?1, id, '2026-06-22T00:00:01.000Z'
             FROM phases
             WHERE text_id = 'move-root-current-phase'",
            [canonical_new_repo.to_string_lossy().as_ref()],
        )
        .expect("insert destination active root");
    db.connection()
        .execute(
            "INSERT INTO phase_ownership
             (phase_id, owner_kind, owner_id, claimed_by_workspace_id, claimed_by_workspace_root, claimed_at, updated_at)
             SELECT id, 'workspace', 'destination-owner', 'destination-owner', ?1,
                    '2026-06-22T00:00:01.000Z', '2026-06-22T00:00:01.000Z'
             FROM phases
             WHERE text_id = 'move-root-current-phase'",
            [canonical_new_repo.to_string_lossy().as_ref()],
        )
        .expect("insert destination phase owner");
    db.connection()
        .execute(
            "INSERT INTO phases (text_id, title, status, epoch_id, kind, slug, sort_key)
             SELECT 'move-root-future-phase', 'Move Root Future Phase', 'pending',
                    id, 'regular', NULL, '00000003'
             FROM epochs
             WHERE text_id = 'move-root-epoch'",
            [],
        )
        .expect("insert future source-owned phase");
    db.connection()
        .execute(
            "DELETE FROM phase_ownership
             WHERE claimed_by_workspace_root = ?1",
            [old_repo.to_string_lossy().as_ref()],
        )
        .expect("delete source owner for completed phase");
    db.connection()
        .execute(
            "INSERT INTO phase_ownership
             (phase_id, owner_kind, owner_id, claimed_by_workspace_id, claimed_by_workspace_root, claimed_at, updated_at)
             SELECT id, 'workspace', 'source-owner', 'source-owner', ?1,
                    '2026-06-22T00:00:02.000Z', '2026-06-22T00:00:02.000Z'
             FROM phases
             WHERE text_id = 'move-root-future-phase'",
            [old_repo.to_string_lossy().as_ref()],
        )
        .expect("insert source owner for pending phase");

    let output = exo_direct_cmd(&old_repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "project",
            "move-root",
            "--key",
            "move-root-test",
            "--to",
            new_repo.to_str().expect("new repo path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["apply_ready"], true);
    let changes = result["changes"].as_array().expect("changes");
    assert!(
        changes.iter().any(|change| {
            change["target"] == "workspace_active_phase.workspace_root"
                && change["action"] == "delete_completed_source"
        }),
        "{result:#?}"
    );
    assert_eq!(
        sidecar_workspace_roots(
            &sidecar_root,
            "move-root-test",
            "workspace_active_phase_data",
            "workspace_root"
        ),
        vec![canonical_new_repo.to_string_lossy().to_string()]
    );
    assert_eq!(
        sidecar_workspace_roots(
            &sidecar_root,
            "move-root-test",
            "phase_ownership_data",
            "claimed_by_workspace_root"
        ),
        vec![canonical_new_repo.to_string_lossy().to_string()]
    );
    assert_eq!(
        sidecar_manifest_project_id(&sidecar_root, "move-root-test"),
        new_project_id
    );
}

#[test]
fn project_move_root_blocks_completed_source_pin_with_in_progress_source_owner() {
    let temp = short_tempdir();
    let old_repo = temp.path().join("old-repo");
    let new_repo = temp.path().join("new-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&old_repo).expect("create old repo");
    std::fs::create_dir_all(&new_repo).expect("create new repo");
    git_init(&old_repo);
    git_init(&new_repo);
    link_sidecar_with_key(
        &old_repo,
        &home,
        &config_home,
        &sidecar_root,
        "move-root-test",
    );
    let canonical_new_repo = new_repo.canonicalize().expect("canonical new repo");
    seed_sidecar_workspace_root_state(&sidecar_root, "move-root-test", &old_repo);

    let db_path = project_state_path(&sidecar_root, "move-root-test", &["cache", "exo.db"]);
    let db = exosuit_storage::open_database(&db_path).expect("open sidecar db");
    db.connection()
        .execute(
            "UPDATE phases
             SET status = 'completed'
             WHERE text_id = 'move-root-phase'",
            [],
        )
        .expect("mark source active phase completed");
    db.connection()
        .execute(
            "INSERT INTO phases (text_id, title, status, epoch_id, kind, slug, sort_key)
             SELECT 'move-root-current-phase', 'Move Root Current Phase', 'in-progress',
                    id, 'regular', NULL, '00000002'
             FROM epochs
             WHERE text_id = 'move-root-epoch'",
            [],
        )
        .expect("insert destination active phase");
    db.connection()
        .execute(
            "INSERT INTO workspace_active_phase (workspace_root, phase_id, updated_at)
             SELECT ?1, id, '2026-06-22T00:00:01.000Z'
             FROM phases
             WHERE text_id = 'move-root-current-phase'",
            [canonical_new_repo.to_string_lossy().as_ref()],
        )
        .expect("insert destination active root");
    db.connection()
        .execute(
            "INSERT INTO phases (text_id, title, status, epoch_id, kind, slug, sort_key)
             SELECT 'move-root-source-owned-phase', 'Move Root Source Owned Phase', 'in-progress',
                    id, 'regular', NULL, '00000003'
             FROM epochs
             WHERE text_id = 'move-root-epoch'",
            [],
        )
        .expect("insert source-owned in-progress phase");
    db.connection()
        .execute(
            "DELETE FROM phase_ownership
             WHERE claimed_by_workspace_root = ?1",
            [old_repo.to_string_lossy().as_ref()],
        )
        .expect("delete source owner for completed phase");
    db.connection()
        .execute(
            "INSERT INTO phase_ownership
             (phase_id, owner_kind, owner_id, claimed_by_workspace_id, claimed_by_workspace_root, claimed_at, updated_at)
             SELECT id, 'workspace', 'source-owner', 'source-owner', ?1,
                    '2026-06-22T00:00:02.000Z', '2026-06-22T00:00:02.000Z'
             FROM phases
             WHERE text_id = 'move-root-source-owned-phase'",
            [old_repo.to_string_lossy().as_ref()],
        )
        .expect("insert source owner for in-progress phase");

    let output = exo_direct_cmd(&old_repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "project",
            "move-root",
            "--key",
            "move-root-test",
            "--to",
            new_repo.to_str().expect("new repo path is utf-8"),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    let message = error["message"].as_str().expect("error message");
    assert!(
        message.contains("source workspace root still owns in-progress phase state"),
        "{message}"
    );
}

#[test]
fn project_move_root_rejects_bare_destination() {
    let temp = short_tempdir();
    let old_repo = temp.path().join("old-repo");
    let bare_repo = temp.path().join("bare.git");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&old_repo).expect("create old repo");
    std::fs::create_dir_all(&bare_repo).expect("create bare repo");
    git_init(&old_repo);
    git_init_bare(&bare_repo);
    link_sidecar_with_key(
        &old_repo,
        &home,
        &config_home,
        &sidecar_root,
        "move-root-test",
    );

    let output = exo_direct_cmd(&old_repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "project",
            "move-root",
            "--key",
            "move-root-test",
            "--to",
            bare_repo.to_str().expect("bare repo path is utf-8"),
            "--dry-run",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    let message = error["message"].as_str().expect("error message");
    assert!(
        message.contains("git repository without a worktree"),
        "{message}"
    );
}

#[test]
fn project_move_root_reports_policy_conflict_before_db_rewrite() {
    let temp = short_tempdir();
    let old_repo = temp.path().join("old-repo");
    let new_repo = temp.path().join("new-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let other_sidecar_root = temp.path().join("other-sidecars");
    std::fs::create_dir_all(&old_repo).expect("create old repo");
    std::fs::create_dir_all(&new_repo).expect("create new repo");
    git_init(&old_repo);
    git_init(&new_repo);
    link_sidecar_with_key(
        &old_repo,
        &home,
        &config_home,
        &sidecar_root,
        "move-root-test",
    );
    let old_project_id = project_id_for(&old_repo, &home, &config_home);
    let new_project_id = project_id_for(&new_repo, &home, &config_home);
    seed_sidecar_workspace_root_state(&sidecar_root, "move-root-test", &old_repo);

    let policy_path = config_home.join("exo").join("projects.toml");
    let mut policy = std::fs::read_to_string(&policy_path).expect("read project policy");
    policy.push_str(&format!(
        "\n[projects.\"{new_project_id}\"]\nstate = \"sidecar\"\nsidecar_key = \"conflicting-key\"\nsidecar_root = \"{}\"\n",
        other_sidecar_root.to_string_lossy()
    ));
    std::fs::write(&policy_path, policy).expect("write conflicting project policy");

    let output = exo_direct_cmd(&old_repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "project",
            "move-root",
            "--key",
            "move-root-test",
            "--to",
            new_repo.to_str().expect("new repo path is utf-8"),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    let message = error["message"].as_str().expect("error message");
    assert!(
        message.contains("already has a different local project policy entry"),
        "{message}"
    );
    assert!(policy_contains_project_id(&config_home, &old_project_id));
    assert_eq!(
        sidecar_workspace_roots(
            &sidecar_root,
            "move-root-test",
            "workspace_active_phase_data",
            "workspace_root"
        ),
        vec![old_repo.to_string_lossy().to_string()]
    );
    assert_eq!(
        sidecar_workspace_roots(
            &sidecar_root,
            "move-root-test",
            "phase_ownership_data",
            "claimed_by_workspace_root"
        ),
        vec![old_repo.to_string_lossy().to_string()]
    );
}

#[test]
fn project_move_root_skips_generic_sidecar_post_write_path() {
    let temp = short_tempdir();
    let old_repo = temp.path().join("old-repo");
    let new_repo = temp.path().join("new-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&old_repo).expect("create old repo");
    std::fs::create_dir_all(&new_repo).expect("create new repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&old_repo);
    git_init(&new_repo);
    git_init(&sidecar_root);
    link_sidecar_with_key(
        &old_repo,
        &home,
        &config_home,
        &sidecar_root,
        "move-root-test",
    );
    let canonical_new_repo = new_repo.canonicalize().expect("canonical new repo");
    seed_sidecar_workspace_root_state(&sidecar_root, "move-root-test", &old_repo);
    let old_rfc_path = create_rfc_00001(&old_repo, &home, &config_home);
    copy_rfc_file_to_new_root(&old_rfc_path, &old_repo, &new_repo);

    let output = exo_cmd(&old_repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "project",
            "move-root",
            "--key",
            "move-root-test",
            "--to",
            new_repo.to_str().expect("new repo path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(
        result["new_workspace_root"].as_str(),
        Some(canonical_new_repo.to_string_lossy().as_ref())
    );
    assert_eq!(result["apply_ready"], true);
    assert_eq!(
        sidecar_workspace_roots(
            &sidecar_root,
            "move-root-test",
            "workspace_active_phase_data",
            "workspace_root"
        ),
        vec![canonical_new_repo.to_string_lossy().to_string()]
    );
}

#[test]
fn project_move_root_refuses_when_destination_has_phase_owner_root() {
    let temp = short_tempdir();
    let old_repo = temp.path().join("old-repo");
    let new_repo = temp.path().join("new-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&old_repo).expect("create old repo");
    std::fs::create_dir_all(&new_repo).expect("create new repo");
    git_init(&old_repo);
    git_init(&new_repo);
    link_sidecar_with_key(
        &old_repo,
        &home,
        &config_home,
        &sidecar_root,
        "move-root-test",
    );
    let canonical_new_repo = new_repo.canonicalize().expect("canonical new repo");
    seed_sidecar_workspace_root_state(&sidecar_root, "move-root-test", &old_repo);

    let db_path = project_state_path(&sidecar_root, "move-root-test", &["cache", "exo.db"]);
    let db = exosuit_storage::open_database(&db_path).expect("open sidecar db");
    db.connection()
        .execute(
            "INSERT INTO phases (text_id, title, status, epoch_id, kind, slug, sort_key)
             SELECT 'move-root-destination-owner', 'Move Root Destination Owner', 'in-progress',
                    id, 'regular', NULL, '00000002'
             FROM epochs
             WHERE text_id = 'move-root-epoch'",
            [],
        )
        .expect("insert destination phase");
    db.connection()
        .execute(
            "INSERT INTO phase_ownership
             (phase_id, owner_kind, owner_id, claimed_by_workspace_id, claimed_by_workspace_root, claimed_at, updated_at)
             SELECT id, 'workspace', 'destination-owner', 'destination-owner', ?1,
                    '2026-06-22T00:00:01.000Z', '2026-06-22T00:00:01.000Z'
             FROM phases
             WHERE text_id = 'move-root-destination-owner'",
            [canonical_new_repo.to_string_lossy().as_ref()],
        )
        .expect("insert destination phase owner");

    let output = exo_direct_cmd(&old_repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "project",
            "move-root",
            "--key",
            "move-root-test",
            "--to",
            new_repo.to_str().expect("new repo path is utf-8"),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    let message = error["message"].as_str().expect("error message");
    assert!(
        message.contains("destination workspace root already has phase ownership state"),
        "{message}"
    );
}

#[test]
fn sidecar_init_defaults_key_root_and_git_repo() {
    let temp = short_tempdir();
    let repo = temp.path().join("External Repo!!");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(repo.join("docs/agent-context")).expect("create repo projection");
    std::fs::write(
        repo.join("docs/agent-context/epochs.sql"),
        "-- Auto-generated by exo. Regenerate: exo status\nINSERT INTO epochs_data(text_id, title, slug, reviewed, sort_key) VALUES('seeded-epoch', 'Seeded Epoch', 'seeded-epoch', 0, '00000000000000000000');\n",
    )
    .expect("write repo projection");
    git_init(&repo);

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "init", "--git"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    let sidecar_root = home.join("exo/sidecars");
    assert_eq!(result["kind"], "sidecar.init");
    assert_eq!(result["sidecar_key"], "external-repo");
    assert_eq!(
        result["sidecar_root"].as_str(),
        Some(sidecar_root.to_str().expect("sidecar root is utf-8"))
    );
    assert_eq!(result["git_initialized"], true);
    assert_eq!(result["seeded_from_repo"], true);
    assert_eq!(result["db_created"], true);
    assert!(sidecar_root.join(".git").exists());
    assert_eq!(
        git_local_config(&sidecar_root, "user.name").as_deref(),
        Some(SIDECAR_GIT_USER_NAME)
    );
    assert_eq!(
        git_local_config(&sidecar_root, "user.email").as_deref(),
        Some(SIDECAR_GIT_USER_EMAIL)
    );
    assert!(
        sidecar_root
            .join("projects/external-repo/sidecar.toml")
            .exists()
    );
    assert!(
        sidecar_root
            .join("projects/external-repo/agent-context")
            .exists()
    );
    assert!(
        sidecar_root
            .join("projects/external-repo/cache/exo.db")
            .exists()
    );
    assert!(!home.join(".exo/sidecars/external-repo").exists());
    let seeded = std::fs::read_to_string(
        sidecar_root.join("projects/external-repo/agent-context/epochs.sql"),
    )
    .expect("read seeded sidecar projection");
    assert!(seeded.contains("Seeded Epoch"));

    let output = exo_direct_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "epoch", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    let epochs = result["epochs"].as_array().expect("epochs array");
    assert!(epochs.iter().any(|epoch| epoch["title"] == "Seeded Epoch"));
    assert!(!repo.join(".exo").exists());
    assert_no_work_repo_daemon_runtime(&repo);
    assert!(!repo.join("exosuit.toml").exists());

    let policy =
        std::fs::read_to_string(config_home.join("exo/projects.toml")).expect("read policy");
    assert!(policy.contains("state = \"sidecar\""));
    assert!(policy.contains("sidecar_key = \"external-repo\""));
}

#[test]
fn sidecar_init_preserves_existing_sidecar_git_identity() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let sidecar_root = temp.path().join("sidecars");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_success(
        &sidecar_root,
        &["config", "--local", "user.name", "Existing Sidecar"],
    );
    git_success(
        &sidecar_root,
        &[
            "config",
            "--local",
            "user.email",
            "existing@example.invalid",
        ],
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "init",
            "--git",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.init");
    assert_eq!(result["git_initialized"], false);
    assert_eq!(
        git_local_config(&sidecar_root, "user.name").as_deref(),
        Some("Existing Sidecar")
    );
    assert_eq!(
        git_local_config(&sidecar_root, "user.email").as_deref(),
        Some("existing@example.invalid")
    );
}

#[test]
fn sidecar_bootstrap_defaults_to_git_and_reports_no_remote_next_action() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    let sidecar_root = home.join("exo/sidecars");
    assert_eq!(result["kind"], "sidecar.bootstrap");
    assert_eq!(result["ok"], true);
    assert_eq!(result["ready"], false);
    assert_eq!(result["sidecar_key"], "external-repo");
    assert_eq!(result["sidecar_root_source"], "default_user_root");
    assert_eq!(
        result["default_sidecar_root"].as_str(),
        Some(sidecar_root.to_str().expect("sidecar root is utf-8"))
    );
    assert_eq!(result["git_initialized"], true);
    assert_eq!(result["repo_clean"], false);
    assert_eq!(result["has_remote"], false, "{result:?}");
    assert!(
        result["sync_issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("uncommitted changes")),
        "{result:?}"
    );
    assert_eq!(
        result["sidecar_root"].as_str(),
        Some(sidecar_root.to_str().expect("sidecar root is utf-8"))
    );
    let known_roots = result["known_sidecar_roots"]
        .as_array()
        .expect("known sidecar roots array");
    assert!(known_roots.iter().any(|root| {
        root["root"].as_str() == Some(sidecar_root.to_str().expect("sidecar root is utf-8"))
            && root["source"] == "default_user_root"
            && root["is_default"] == true
    }));
    assert!(sidecar_root.join(".git").exists());
    assert_eq!(
        git_local_config(&sidecar_root, "user.name").as_deref(),
        Some(SIDECAR_GIT_USER_NAME)
    );
    assert_eq!(
        git_local_config(&sidecar_root, "user.email").as_deref(),
        Some(SIDECAR_GIT_USER_EMAIL)
    );
    assert!(
        sidecar_root
            .join("projects/external-repo/sidecar.toml")
            .exists()
    );
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions.iter().any(|action| {
            action["command"]
                == "exo sidecar repo commit --message \"Bootstrap Exosuit sidecar state\""
        })
    }));
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions
            .iter()
            .any(|action| action["command"] == "exo sidecar repo remote --url <url>")
    }));
    assert_no_work_repo_daemon_runtime(&repo);
    assert_eq!(git_status_porcelain(&repo), "");
}

#[test]
fn sidecar_bootstrap_plugin_after_git_init_uses_default_user_root() {
    let temp = short_tempdir();
    let repo = temp.path().join("visible-browser-lab");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--key",
            "visible-browser-lab",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    let default_root = home.join("exo/sidecars");

    assert_eq!(result["kind"], "sidecar.bootstrap");
    assert_eq!(result["ok"], true);
    assert_eq!(result["sidecar_key"], "visible-browser-lab");
    assert_eq!(result["sidecar_root_source"], "default_user_root");
    assert_eq!(
        result["sidecar_root"].as_str(),
        Some(default_root.to_str().expect("default root is utf-8"))
    );
    assert_eq!(
        result["default_sidecar_root"].as_str(),
        Some(default_root.to_str().expect("default root is utf-8"))
    );
    assert!(
        default_root
            .join("projects/visible-browser-lab/sidecar.toml")
            .exists()
    );
    assert!(
        result["known_sidecar_roots"]
            .as_array()
            .is_some_and(|roots| {
                roots.iter().any(|root| {
                    root["root"].as_str()
                        == Some(default_root.to_str().expect("default root is utf-8"))
                        && root["source"] == "default_user_root"
                        && root["is_default"] == true
                })
            })
    );

    let human_output = exo_cmd(&repo, &home, &config_home)
        .args(["sidecar", "bootstrap", "--key", "visible-browser-lab"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(human_output).expect("human output is utf-8");
    assert!(
        human.contains("Root source: existing project root")
            || human.contains("Root source: default user sidecar root"),
        "{human}"
    );
    assert!(human.contains("Default user root:"), "{human}");
}

#[test]
fn sidecar_bootstrap_explicit_root_does_not_require_home_for_reporting() {
    let temp = short_tempdir();
    let repo = temp.path().join("visible-browser-lab");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("explicit-sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    let output = exo_cmd(&repo, Path::new("/unused-home"), &config_home)
        .env_remove("HOME")
        .env_remove("USERPROFILE")
        .env_remove("APPDATA")
        .env_remove("HOMEDRIVE")
        .env_remove("HOMEPATH")
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--key",
            "visible-browser-lab",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.bootstrap");
    assert_eq!(result["ok"], true);
    assert_eq!(result["sidecar_root_source"], "explicit");
    assert_eq!(
        result["sidecar_root"].as_str(),
        Some(sidecar_root.to_str().expect("sidecar root is utf-8"))
    );
    assert!(result["default_sidecar_root"].is_null(), "{result:?}");
    assert!(
        result["known_sidecar_roots"]
            .as_array()
            .is_some_and(|roots| {
                roots.iter().any(|root| {
                    root["root"].as_str()
                        == Some(sidecar_root.to_str().expect("sidecar root is utf-8"))
                        && root["source"] == "existing_project_root"
                        && root["is_default"] == false
                })
            })
    );
}

#[test]
fn sidecar_bootstrap_without_root_reports_sidecar_default_home_error() {
    let temp = short_tempdir();
    let repo = temp.path().join("visible-browser-lab");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    let output = exo_cmd(&repo, Path::new("/unused-home"), &config_home)
        .env_remove("HOME")
        .env_remove("USERPROFILE")
        .env_remove("APPDATA")
        .env_remove("HOMEDRIVE")
        .env_remove("HOMEPATH")
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--key",
            "visible-browser-lab",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("Default sidecar root requires")),
        "{error:?}"
    );
}

#[test]
fn sidecar_bootstrap_reports_existing_project_roots_separately_from_default_root() {
    let temp = short_tempdir();
    let repo = temp.path().join("visible-browser-lab");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let existing_root = temp.path().join("existing-sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(config_home.join("exo")).expect("create config home");
    git_init(&repo);
    let existing_root_literal = existing_root
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    std::fs::write(
        config_home.join("exo/projects.toml"),
        format!(
            r#"[projects.existing_project]
state = "sidecar"
sidecar_key = "existing-plugin"
sidecar_root = "{existing_root_literal}"
"#
        ),
    )
    .expect("write project policy");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--key",
            "visible-browser-lab",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    let default_root = home.join("exo/sidecars");

    assert_eq!(result["sidecar_root_source"], "default_user_root");
    assert_eq!(
        result["sidecar_root"].as_str(),
        Some(default_root.to_str().expect("default root is utf-8"))
    );
    let known_roots = result["known_sidecar_roots"]
        .as_array()
        .expect("known sidecar roots array");
    assert!(known_roots.iter().any(|root| {
        root["root"].as_str() == Some(default_root.to_str().expect("default root is utf-8"))
            && root["source"] == "default_user_root"
            && root["is_default"] == true
    }));
    assert!(known_roots.iter().any(|root| {
        root["root"].as_str() == Some(existing_root.to_str().expect("existing root is utf-8"))
            && root["source"] == "existing_project_root"
            && root["is_default"] == false
            && root["project_keys"]
                .as_array()
                .is_some_and(|keys| keys.iter().any(|key| key == "existing-plugin"))
    }));

    let human_output = exo_cmd(&repo, &home, &config_home)
        .args(["sidecar", "bootstrap", "--key", "visible-browser-lab"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(human_output).expect("human output is utf-8");
    assert!(
        human.contains("Root source: existing project root")
            || human.contains("Root source: default user sidecar root"),
        "{human}"
    );
    assert!(human.contains("Existing project roots:"), "{human}");
    assert!(human.contains("existing-plugin"), "{human}");
}

#[test]
fn sidecar_bootstrap_reports_whole_repo_cleanliness_with_foreign_debt() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success();
    git_config_identity(&home.join("exo/sidecars"));
    exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Bootstrap sidecar",
        ])
        .assert()
        .success();

    let sidecar_root = home.join("exo/sidecars");
    let foreign_path = sidecar_root.join("projects/sandboxd/agent-context/tasks.sql");
    std::fs::create_dir_all(foreign_path.parent().expect("foreign parent"))
        .expect("create foreign projection dir");
    std::fs::write(&foreign_path, "sandboxd task debt\n").expect("write foreign debt");

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.bootstrap");
    assert_eq!(result["ready"], false, "{result:?}");
    assert_eq!(result["repo_clean"], false, "{result:?}");
    assert!(
        result["sync_issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("foreign or cross-project checkpoint debt")),
        "{result:?}"
    );
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions
            .iter()
            .any(|action| action["command"] == "exo sidecar checkpoint --project sandboxd")
    }));
}

#[test]
fn sidecar_bootstrap_reports_loose_repo_dirt_repair_action() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let remote = temp.path().join("sidecars.git");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&remote).expect("create remote");
    git_init(&repo);
    git_init_bare(&remote);

    exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success();
    let sidecar_root = home.join("exo/sidecars");
    git_config_identity(&sidecar_root);
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path is utf-8"),
        ],
    );
    exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Bootstrap sidecar",
        ])
        .assert()
        .success();
    git_success(&sidecar_root, &["push", "-u", "origin", "HEAD"]);

    std::fs::write(sidecar_root.join("loose.txt"), "dirty\n").expect("write loose repo dirt");

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    let next_actions = result["next_actions"]
        .as_array()
        .expect("next actions array");

    assert_eq!(result["kind"], "sidecar.bootstrap");
    assert_eq!(result["ready"], false, "{result:?}");
    assert_eq!(result["repo_clean"], false, "{result:?}");
    assert!(
        result["sync_issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("uncommitted changes")),
        "{result:?}"
    );
    assert!(
        next_actions
            .iter()
            .any(|action| action["command"] == "exo sidecar repo status"),
        "{result:?}"
    );
    assert!(
        !next_actions.iter().any(|action| {
            action["command"]
                == "exo sidecar repo commit --message \"Bootstrap Exosuit sidecar state\""
        }),
        "{result:?}"
    );
}

#[test]
fn sidecar_bootstrap_is_idempotent_and_reports_no_remote_after_commit() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success();
    git_config_identity(&home.join("exo/sidecars"));
    exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Bootstrap sidecar",
        ])
        .assert()
        .success();

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.bootstrap");
    assert_eq!(result["ok"], true);
    assert_eq!(result["ready"], false);
    assert_eq!(result["git_initialized"], false);
    assert_eq!(result["db_created"], false);
    assert_eq!(result["repo_clean"], true);
    assert_eq!(result["has_remote"], false, "{result:?}");
    assert!(
        result["sync_issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("no remote")),
        "{result:?}"
    );
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions
            .iter()
            .any(|action| action["command"] == "exo sidecar repo remote --url <url>")
    }));
    assert_eq!(git_status_porcelain(&repo), "");
}

#[test]
fn sidecar_repo_status_marks_clean_repo_without_remote_unsyncable() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success();
    git_config_identity(&home.join("exo/sidecars"));
    exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Bootstrap sidecar",
        ])
        .assert()
        .success();

    let output = exo_direct_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.repo.status");
    assert_eq!(result["ok"], true, "{result:?}");
    assert_eq!(result["clean"], true, "{result:?}");
    assert_eq!(result["has_remote"], false, "{result:?}");
    assert_eq!(result["syncable"], false, "{result:?}");
    assert_eq!(
        result["issue_kind"].as_str(),
        Some("no_remote"),
        "{result:?}"
    );
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions
            .iter()
            .any(|action| action["command"].as_str() == Some("exo sidecar repo remote --url <url>"))
    }));
}

#[test]
fn sidecar_bootstrap_discover_applies_exact_project_binding_without_remote_acceptance() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let sidecar_root = temp.path().join("discovered-sidecars");
    let remote = seeded_sidecar_remote(
        temp.path(),
        "locald-state",
        &[(
            "projects/sandboxd/agent-context/tasks.sql",
            "sandboxd task debt\n",
        )],
    );
    let remote_url = "https://github.com/wycats/locald-exosuit-state.git";
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald-state"
root = {:?}
remote = {:?}
auto_push = "if_remote"
"#,
            sidecar_root.to_str().expect("sidecar root is utf-8"),
            remote_url
        ),
    )
    .expect("write registry");

    let mut cmd = exo_cmd(&repo, &home, &config_home);
    add_git_remote_rewrite(&mut cmd, remote_url, &remote);
    let output = cmd
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.bootstrap");
    assert_eq!(result["sidecar_key"], "locald-state");
    assert_eq!(result["has_remote"], true);
    assert_eq!(result["remote"], "origin");
    assert_eq!(
        result["sidecar_root"].as_str(),
        Some(sidecar_root.to_str().expect("sidecar root is utf-8"))
    );
    assert_eq!(result["discovery"]["ok"], true);
    assert_eq!(result["discovery"]["match"]["kind"], "exact-project");
    assert!(
        sidecar_root
            .join("projects/locald-state/sidecar.toml")
            .exists()
    );
    assert_eq!(
        git_output(&sidecar_root, &["remote", "get-url", "origin"]),
        format!("{remote_url}\n")
    );
    assert!(
        sidecar_root
            .join("projects/sandboxd/agent-context/tasks.sql")
            .exists()
    );
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        !actions.iter().any(|action| {
            action["command"]
                == "exo sidecar repo remote --url git@github.com:wycats/locald-exosuit-state.git"
        })
    }));

    let policy = std::fs::read_to_string(config_home.join("exo/projects.toml"))
        .expect("read project policy");
    assert!(policy.contains("sidecar_key = \"locald-state\""));
    assert_eq!(
        policy_value_for_sidecar_key(&config_home, "locald-state", "sidecar_root"),
        sidecar_root.to_str().expect("sidecar root is utf-8")
    );
}

#[test]
fn sidecar_bootstrap_discover_clones_missing_discovered_sidecar_root() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let sidecar_root = temp.path().join("missing-sidecars");
    let remote = seeded_sidecar_remote(
        temp.path(),
        "missing-sidecars",
        &[(
            "projects/sandboxd/agent-context/inbox.sql",
            "sandboxd inbox\n",
        )],
    );
    let remote_url = "https://github.com/wycats/missing-sidecars.git";
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald"
root = {:?}
remote = {:?}
auto_push = "if_remote"
"#,
            sidecar_root.to_str().expect("sidecar root is utf-8"),
            remote_url
        ),
    )
    .expect("write registry");

    let mut cmd = exo_cmd(&repo, &home, &config_home);
    add_git_remote_rewrite(&mut cmd, remote_url, &remote);
    let output = cmd
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.bootstrap");
    assert_eq!(result["sidecar_key"], "locald");
    assert_eq!(result["has_remote"], true);
    assert_eq!(
        git_output(&sidecar_root, &["branch", "--show-current"]),
        "main\n"
    );
    assert_eq!(
        git_output(&sidecar_root, &["remote", "get-url", "origin"]),
        format!("{remote_url}\n")
    );
    assert!(
        sidecar_root
            .join("projects/sandboxd/agent-context/inbox.sql")
            .exists()
    );
    assert!(sidecar_root.join("projects/locald/sidecar.toml").exists());
}

#[test]
fn sidecar_bootstrap_discover_clones_into_existing_empty_sidecar_root() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let sidecar_root = temp.path().join("empty-sidecars");
    let remote = seeded_sidecar_remote(temp.path(), "empty-sidecars", &[("README.md", "hub\n")]);
    let remote_url = "https://github.com/wycats/empty-sidecars.git";
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create empty sidecar root");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald"
root = {:?}
remote = {:?}
auto_push = "if_remote"
"#,
            sidecar_root.to_str().expect("sidecar root is utf-8"),
            remote_url
        ),
    )
    .expect("write registry");

    let mut cmd = exo_cmd(&repo, &home, &config_home);
    add_git_remote_rewrite(&mut cmd, remote_url, &remote);
    cmd.args([
        "--format",
        "json",
        "sidecar",
        "bootstrap",
        "--discover",
        "--registry-file",
        registry.to_str().expect("registry path is utf-8"),
    ])
    .assert()
    .success();

    assert_eq!(
        git_output(&sidecar_root, &["remote", "get-url", "origin"]),
        format!("{remote_url}\n")
    );
    assert!(sidecar_root.join("README.md").exists());
    assert!(sidecar_root.join("projects/locald/sidecar.toml").exists());
}

#[test]
fn sidecar_bootstrap_discover_fetches_existing_related_sidecar_root_before_writes() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let sidecar_root = temp.path().join("related-sidecars");
    let remote = seeded_sidecar_remote(temp.path(), "related-sidecars", &[("README.md", "hub\n")]);
    let remote_url = "https://github.com/wycats/related-sidecars.git";
    let seeder = temp.path().join("related-seeder");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_success(
        temp.path(),
        &[
            "clone",
            remote.to_str().expect("remote path is utf-8"),
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ],
    );
    git_success(&sidecar_root, &["remote", "set-url", "origin", remote_url]);
    git_success(
        temp.path(),
        &[
            "clone",
            remote.to_str().expect("remote path is utf-8"),
            seeder.to_str().expect("seeder path is utf-8"),
        ],
    );
    git_config_identity(&sidecar_root);
    git_config_identity(&seeder);
    let remote_file = seeder.join("projects/open-wc/agent-context/tasks.sql");
    std::fs::create_dir_all(remote_file.parent().expect("remote file parent"))
        .expect("create remote file parent");
    std::fs::write(&remote_file, "open-wc task\n").expect("write remote file");
    git_success(&seeder, &["add", "-A"]);
    git_success(&seeder, &["commit", "-m", "Add open-wc state"]);
    git_success(&seeder, &["push", "origin", "main"]);
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald"
root = {:?}
remote = {:?}
auto_push = "if_remote"
"#,
            sidecar_root.to_str().expect("sidecar root is utf-8"),
            remote_url
        ),
    )
    .expect("write registry");

    let mut cmd = exo_cmd(&repo, &home, &config_home);
    add_git_remote_rewrite(&mut cmd, remote_url, &remote);
    cmd.args([
        "--format",
        "json",
        "sidecar",
        "bootstrap",
        "--discover",
        "--registry-file",
        registry.to_str().expect("registry path is utf-8"),
    ])
    .assert()
    .success();

    assert!(
        sidecar_root
            .join("projects/open-wc/agent-context/tasks.sql")
            .exists()
    );
    assert!(sidecar_root.join("projects/locald/sidecar.toml").exists());
}

#[test]
fn sidecar_bootstrap_discover_refuses_unrelated_existing_root_before_writes() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let sidecar_root = temp.path().join("unrelated-sidecars");
    let remote =
        seeded_sidecar_remote(temp.path(), "unrelated-sidecars", &[("README.md", "hub\n")]);
    let remote_url = "https://github.com/wycats/unrelated-sidecars.git";
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    std::fs::write(sidecar_root.join("local.md"), "local\n").expect("write local file");
    git_success(&sidecar_root, &["add", "-A"]);
    git_success(&sidecar_root, &["commit", "-m", "Local sidecar"]);
    git_success(&sidecar_root, &["remote", "add", "origin", remote_url]);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald"
root = {:?}
remote = {:?}
auto_push = "if_remote"
"#,
            sidecar_root.to_str().expect("sidecar root is utf-8"),
            remote_url
        ),
    )
    .expect("write registry");

    let mut cmd = exo_cmd(&repo, &home, &config_home);
    add_git_remote_rewrite(&mut cmd, remote_url, &remote);
    let output = cmd
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("unrelated history")),
        "{error:?}"
    );
    assert!(!config_home.join("exo/projects.toml").exists());
    assert!(!sidecar_root.join("projects/locald/sidecar.toml").exists());
}

#[test]
fn sidecar_bootstrap_discover_refuses_non_git_non_empty_root_before_writes() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let sidecar_root = temp.path().join("non-git-sidecars");
    let remote_url = "https://github.com/wycats/non-git-sidecars.git";
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::write(sidecar_root.join("local.txt"), "local\n").expect("write local file");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald"
root = {:?}
remote = {:?}
auto_push = "if_remote"
"#,
            sidecar_root.to_str().expect("sidecar root is utf-8"),
            remote_url
        ),
    )
    .expect("write registry");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("not an independent git repository")),
        "{error:?}"
    );
    assert!(!config_home.join("exo/projects.toml").exists());
    assert!(!sidecar_root.join("projects/locald/sidecar.toml").exists());
}

#[test]
fn sidecar_bootstrap_discover_expands_tilde_root_before_writing_policy() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let expanded_root = home.join(".exo/sidecars");
    let remote = seeded_sidecar_remote(temp.path(), "tilde-sidecars", &[]);
    let remote_url = "https://github.com/wycats/tilde-sidecars.git";
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald"
root = "~/.exo/sidecars"
remote = {:?}
auto_push = "if_remote"
"#,
            remote_url
        ),
    )
    .expect("write registry");

    let mut cmd = exo_cmd(&repo, &home, &config_home);
    add_git_remote_rewrite(&mut cmd, remote_url, &remote);
    let output = cmd
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.bootstrap");
    assert_eq!(result["sidecar_key"], "locald");
    assert_eq!(result["discovery"]["proposal"]["root"], "~/.exo/sidecars");
    assert_eq!(
        result["sidecar_root"].as_str(),
        Some(expanded_root.to_str().expect("expanded root is utf-8"))
    );
    assert!(expanded_root.join("projects/locald/sidecar.toml").exists());
    assert!(!repo.join("~").exists());

    let policy = std::fs::read_to_string(config_home.join("exo/projects.toml"))
        .expect("read project policy");
    assert!(policy.contains("sidecar_key = \"locald\""));
    assert_eq!(
        policy_value_for_sidecar_key(&config_home, "locald", "sidecar_root"),
        expanded_root.to_str().expect("expanded root is utf-8")
    );
    assert!(!policy.contains("sidecar_root = \"~/.exo/sidecars\""));
}

#[test]
fn sidecar_bootstrap_discover_rejects_tilde_root_without_home_before_writing_policy() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&config_home).expect("create config home");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald"
root = "~/.exo/sidecars"
remote = "git@github.com:wycats/locald-exosuit-state.git"
auto_push = "if_remote"
"#,
    )
    .expect("write registry");

    let output = exo_cmd(&repo, Path::new("/unused-home"), &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .env_remove("HOME")
        .env_remove("USERPROFILE")
        .env_remove("HOMEDRIVE")
        .env_remove("HOMEPATH")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    assert!(
        error["message"].as_str().is_some_and(|message| {
            message.contains("discovered sidecar root uses ~ but no home directory is available")
        }),
        "{error:?}"
    );
    assert!(!config_home.join("exo/projects.toml").exists());
    assert!(!repo.join("~").exists());
}

#[test]
fn sidecar_bootstrap_discover_rejects_tilde_root_with_empty_home_before_writing_policy() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&config_home).expect("create config home");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald"
root = "~/.exo/sidecars"
remote = "git@github.com:wycats/locald-exosuit-state.git"
auto_push = "if_remote"
"#,
    )
    .expect("write registry");

    let output = exo_cmd(&repo, Path::new("/unused-home"), &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .env("HOME", "")
        .env_remove("USERPROFILE")
        .env_remove("HOMEDRIVE")
        .env_remove("HOMEPATH")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    assert!(
        error["message"].as_str().is_some_and(|message| {
            message.contains("discovered sidecar root uses ~ but no home directory is available")
        }),
        "{error:?}"
    );
    assert!(!config_home.join("exo/projects.toml").exists());
    assert!(!repo.join(".exo/sidecars").exists());
    assert!(!repo.join("~").exists());
}

#[cfg(windows)]
#[test]
fn sidecar_bootstrap_discover_expands_tilde_with_windows_userprofile() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("windows-home");
    let appdata = temp.path().join("roaming");
    let registry = temp.path().join("sidecars.toml");
    let remote = seeded_sidecar_remote(temp.path(), "windows-userprofile-sidecars", &[]);
    let remote_url = "https://github.com/wycats/windows-userprofile-sidecars.git";
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&home).expect("create home");
    std::fs::create_dir_all(&appdata).expect("create appdata");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald"
root = "~/.exo/sidecars"
remote = {:?}
auto_push = "if_remote"
"#,
            remote_url
        ),
    )
    .expect("write registry");

    let mut cmd = exo_cmd(
        &repo,
        Path::new("/unused-home"),
        Path::new("/unused-config"),
    );
    add_git_remote_rewrite(&mut cmd, remote_url, &remote);
    let output = cmd
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .env_remove("HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env("USERPROFILE", &home)
        .env("APPDATA", &appdata)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    let sidecar_root = home.join(".exo/sidecars");
    assert_eq!(result["kind"], "sidecar.bootstrap");
    assert_eq!(result["sidecar_key"], "locald");
    assert_eq!(
        result["sidecar_root"].as_str(),
        Some(sidecar_root.to_str().expect("sidecar root is utf-8"))
    );
    assert!(appdata.join("exo/projects.toml").exists());
    assert!(sidecar_root.join("projects/locald/sidecar.toml").exists());
    assert!(!repo.join("~").exists());
}

#[test]
fn sidecar_bootstrap_discover_explicit_root_overrides_discovered_tilde_root() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let explicit_root = temp.path().join("explicit-sidecars");
    let remote = seeded_sidecar_remote(temp.path(), "explicit-sidecars", &[]);
    let remote_url = "https://github.com/wycats/explicit-sidecars.git";
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&config_home).expect("create config home");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald"
root = "~/.exo/sidecars"
remote = {:?}
auto_push = "if_remote"
"#,
            remote_url
        ),
    )
    .expect("write registry");

    let mut cmd = exo_cmd(&repo, &home, &config_home);
    add_git_remote_rewrite(&mut cmd, remote_url, &remote);
    let output = cmd
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
            "--root",
            explicit_root.to_str().expect("explicit root is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["discovery"]["proposal"]["root"], "~/.exo/sidecars");
    assert_eq!(
        result["sidecar_root"].as_str(),
        Some(explicit_root.to_str().expect("explicit root is utf-8"))
    );
    assert!(explicit_root.join("projects/locald/sidecar.toml").exists());
    assert!(!repo.join("~").exists());

    let policy = std::fs::read_to_string(config_home.join("exo/projects.toml"))
        .expect("read project policy");
    assert_eq!(
        policy_value_for_sidecar_key(&config_home, "locald", "sidecar_root"),
        explicit_root.to_str().expect("explicit root is utf-8")
    );
    assert!(!policy.contains("sidecar_root = \"~/.exo/sidecars\""));
}

#[test]
fn sidecar_bootstrap_discover_no_git_does_not_apply_discovered_remote() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald"
root = {:?}
remote = "git@github.com:wycats/locald-exosuit-state.git"
auto_push = "if_remote"
"#,
            sidecar_root.to_str().expect("sidecar root is utf-8")
        ),
    )
    .expect("write registry");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--no-git",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["sidecar_key"], "locald");
    assert_eq!(result["git_initialized"], false);
    assert_eq!(result["has_remote"], false);
    assert_eq!(
        result["sync_issue"].as_str(),
        Some("sidecar git initialization skipped")
    );
    assert!(!sidecar_root.join(".git").exists());
}

#[test]
fn sidecar_bootstrap_discover_refuses_existing_non_origin_sidecar_remote() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "backup",
            "git@github.com:wycats/existing-sidecar.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald"
root = {:?}
remote = "git@github.com:wycats/locald-exosuit-state.git"
auto_push = "if_remote"
"#,
            sidecar_root.to_str().expect("sidecar root is utf-8")
        ),
    )
    .expect("write registry");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    assert!(
        error["message"].as_str().is_some_and(|message| {
            message.contains("sidecar repo already has remote 'backup'")
        }),
        "{error:?}"
    );
    assert_eq!(git_output(&sidecar_root, &["remote"]), "backup\n");
    assert_eq!(
        git_output(&sidecar_root, &["remote", "get-url", "backup"]),
        "git@github.com:wycats/existing-sidecar.git\n"
    );
    assert!(!config_home.join("exo/projects.toml").exists());
}

#[test]
fn sidecar_bootstrap_discover_refuses_conflicting_origin_before_writing_policy() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/existing-sidecar.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald"
root = {:?}
remote = "git@github.com:wycats/locald-exosuit-state.git"
auto_push = "if_remote"
"#,
            sidecar_root.to_str().expect("sidecar root is utf-8")
        ),
    )
    .expect("write registry");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    assert!(
        error["message"].as_str().is_some_and(|message| {
            message.contains("sidecar repo remote 'origin' already points")
        }),
        "{error:?}"
    );
    assert_eq!(git_output(&sidecar_root, &["remote"]), "origin\n");
    assert_eq!(
        git_output(&sidecar_root, &["remote", "get-url", "origin"]),
        "git@github.com:wycats/existing-sidecar.git\n"
    );
    assert!(!config_home.join("exo/projects.toml").exists());
}

#[test]
fn sidecar_bootstrap_discover_refuses_matching_origin_with_additional_remote() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald-exosuit-state.git",
        ],
    );
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "zzz",
            "git@github.com:wycats/zzz-sidecar.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald"
root = {:?}
remote = "git@github.com:wycats/locald-exosuit-state.git"
auto_push = "if_remote"
"#,
            sidecar_root.to_str().expect("sidecar root is utf-8")
        ),
    )
    .expect("write registry");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| { message.contains("sidecar repo already has remote 'zzz'") }),
        "{error:?}"
    );
    assert_eq!(git_output(&sidecar_root, &["remote"]), "origin\nzzz\n");
    assert!(!config_home.join("exo/projects.toml").exists());
}

#[test]
fn sidecar_status_ignores_inherited_workspace_remote_for_nested_sidecar_root() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let sidecar_root = repo.join("nested-sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald"
root = {:?}
remote = "git@github.com:wycats/locald-exosuit-state.git"
auto_push = "if_remote"
"#,
            sidecar_root.to_str().expect("sidecar root is utf-8")
        ),
    )
    .expect("write registry");

    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "status",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["linked"], true);
    assert_eq!(result["discovery"]["ok"], true);
    assert_eq!(result["discovery"]["match"]["kind"], "exact-project");
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions.iter().any(|action| {
            action["command"]
                == "exo sidecar repo remote --url git@github.com:wycats/locald-exosuit-state.git"
        })
    }));
}

#[cfg(unix)]
#[test]
fn sidecar_bootstrap_discover_fetches_authenticated_user_profile_registry() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("profile-sidecars");
    let remote = seeded_sidecar_remote(temp.path(), "profile-sidecars", &[]);
    let remote_url = "https://github.com/wycats/profile-sidecars.git";
    let registry = format!(
        r#"version = 1

[defaults]
auto_push = "if_remote"

[projects."github.com/wycats/locald"]
key = "locald-profile"
root = {:?}
remote = {:?}
"#,
        sidecar_root.to_str().expect("sidecar root is utf-8"),
        remote_url
    );
    let fake_path = fake_gh_path(
        temp.path(),
        &format!(
            r#"#!/bin/sh
set -eu
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "user" ] && [ "$3" = "--jq" ] && [ "$4" = ".login" ]; then
    echo "alice"
    exit 0
fi
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "-H" ] && [ "$4" = "repos/alice/alice/contents/.exosuit/sidecars.toml" ]; then
    cat <<'EOF'
{registry}
EOF
    exit 0
fi
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "users/wycats" ] && [ "$3" = "--jq" ] && [ "$4" = ".type" ]; then
    echo "User"
    exit 0
fi
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "-H" ] && [ "$4" = "repos/wycats/wycats/contents/.exosuit/sidecars.toml" ]; then
    echo "HTTP 404: Not Found" >&2
    exit 1
fi
echo "unexpected gh invocation: $*" >&2
exit 2
"#
        ),
    );
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );

    let mut cmd = exo_cmd(&repo, &home, &config_home);
    add_git_remote_rewrite(&mut cmd, remote_url, &remote);
    let output = cmd
        .args(["--format", "json", "sidecar", "bootstrap", "--discover"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                fake_path.to_str().expect("fake PATH is utf-8"),
                std::env::var("PATH").expect("PATH is set")
            ),
        )
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.bootstrap");
    assert_eq!(result["ok"], true);
    assert_eq!(result["sidecar_key"], "locald-profile");
    assert_eq!(
        result["sidecar_root"].as_str(),
        Some(sidecar_root.to_str().expect("sidecar root is utf-8"))
    );
    assert_eq!(result["discovery"]["ok"], true);
    assert_eq!(
        result["discovery"]["identity"]["source"],
        "authenticated-user"
    );
    assert_eq!(result["discovery"]["identity"]["login"], "alice");
    assert_eq!(result["discovery"]["registry"]["source"], "github-profile");
    assert_eq!(
        result["discovery"]["registry"]["profile_repo"],
        "github.com/alice/alice"
    );
    assert_eq!(result["discovery"]["attempt_index"], 0);
    assert_eq!(result["discovery"]["checked"][0]["status"], "loaded-match");
    assert_eq!(
        result["discovery"]["match"]["key"],
        "github.com/wycats/locald"
    );
    assert_eq!(result["discovery"]["proposal"]["remote"], remote_url);
    assert_eq!(result["discovery"]["proposal"]["auto_push"], "if_remote");
    assert_eq!(result["repo_clean"], false);
    assert_eq!(result["has_remote"], true);
    assert_eq!(result["remote"], "origin");
    assert_eq!(
        git_output(&sidecar_root, &["remote", "get-url", "origin"]),
        format!("{remote_url}\n")
    );
    assert!(
        sidecar_root
            .join("projects/locald-profile/sidecar.toml")
            .exists()
    );

    let policy = std::fs::read_to_string(config_home.join("exo/projects.toml"))
        .expect("read project policy");
    assert!(policy.contains("sidecar_key = \"locald-profile\""));
    assert!(policy.contains("auto_push = \"if_remote\""));
    assert!(policy.contains(&format!(
        "sidecar_root = {:?}",
        sidecar_root.to_str().expect("sidecar root is utf-8")
    )));
    assert_eq!(git_status_porcelain(&repo), "");

    let human_repo = temp.path().join("locald-human");
    let human_home = temp.path().join("home-human");
    let human_config_home = temp.path().join("config-human");
    let human_sidecar_root = temp.path().join("profile-sidecars-human");
    let human_registry = format!(
        r#"version = 1

[defaults]
auto_push = "if_remote"

[projects."github.com/wycats/locald"]
key = "locald-profile-human"
root = {:?}
remote = {:?}
"#,
        human_sidecar_root.to_str().expect("sidecar root is utf-8"),
        remote_url
    );
    let human_fake_path = fake_gh_path(
        &temp.path().join("human-fake"),
        &format!(
            r#"#!/bin/sh
set -eu
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "user" ] && [ "$3" = "--jq" ] && [ "$4" = ".login" ]; then
    echo "alice"
    exit 0
fi
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "-H" ] && [ "$4" = "repos/alice/alice/contents/.exosuit/sidecars.toml" ]; then
    cat <<'EOF'
{human_registry}
EOF
    exit 0
fi
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "users/wycats" ] && [ "$3" = "--jq" ] && [ "$4" = ".type" ]; then
    echo "User"
    exit 0
fi
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "-H" ] && [ "$4" = "repos/wycats/wycats/contents/.exosuit/sidecars.toml" ]; then
    echo "HTTP 404: Not Found" >&2
    exit 1
fi
echo "unexpected gh invocation: $*" >&2
exit 2
"#
        ),
    );
    std::fs::create_dir_all(&human_repo).expect("create repo");
    git_init(&human_repo);
    git_success(
        &human_repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );

    let mut human_cmd = exo_cmd(&human_repo, &human_home, &human_config_home);
    add_git_remote_rewrite(&mut human_cmd, remote_url, &remote);
    let human_output = human_cmd
        .args(["sidecar", "bootstrap", "--discover"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                human_fake_path.to_str().expect("fake PATH is utf-8"),
                std::env::var("PATH").expect("PATH is set")
            ),
        )
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(human_output).expect("human output is utf-8");
    assert!(human.contains("Sidecar bootstrap ready at"), "{human}");
    assert!(
        human.contains("Discovery: github-profile:.exosuit/sidecars.toml"),
        "{human}"
    );
    assert!(
        human.contains("Discovery location: github.com/alice/alice:.exosuit/sidecars.toml"),
        "{human}"
    );
    assert!(
        human.contains("Source: authenticated-user alice"),
        "{human}"
    );
}

#[test]
fn sidecar_discovery_exact_project_flow_applies_binding_and_status_remote_action() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let sidecar_root = temp.path().join("sidecars");
    let remote = seeded_sidecar_remote(temp.path(), "exact-flow-sidecars", &[]);
    let remote_url = "https://github.com/wycats/exact-flow-sidecars.git";
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[defaults]
root = "~/.exo/sidecars"
remote_template = "git@github.com:{{owner}}/{{repo}}-default.git"
auto_push = "never"

[projects."github.com/wycats/locald"]
key = "locald-exact"
root = {:?}
remote = {:?}
auto_push = "if_remote"
"#,
            sidecar_root.to_str().expect("sidecar root is utf-8"),
            remote_url
        ),
    )
    .expect("write registry");

    let discover_output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let discover = json_result(&discover_output);
    assert_eq!(discover["ok"], true);
    assert_eq!(discover["match"]["kind"], "exact-project");
    assert_eq!(discover["confidence"], "high");
    assert_eq!(discover["proposal"]["key"], "locald-exact");
    assert_eq!(discover["proposal"]["remote"], remote_url);
    assert_eq!(discover["proposal"]["requires_remote_acceptance"], false);

    let mut bootstrap_cmd = exo_cmd(&repo, &home, &config_home);
    add_git_remote_rewrite(&mut bootstrap_cmd, remote_url, &remote);
    let bootstrap_output = bootstrap_cmd
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let bootstrap = json_result(&bootstrap_output);
    assert_eq!(bootstrap["sidecar_key"], "locald-exact");
    assert_eq!(bootstrap["discovery"]["match"]["kind"], "exact-project");
    assert_eq!(bootstrap["discovery"]["proposal"]["auto_push"], "if_remote");
    assert_eq!(
        bootstrap["sidecar_root"].as_str(),
        Some(sidecar_root.to_str().expect("sidecar root is utf-8"))
    );

    let policy = std::fs::read_to_string(config_home.join("exo/projects.toml"))
        .expect("read project policy");
    assert!(policy.contains("sidecar_key = \"locald-exact\""));
    assert!(policy.contains("auto_push = \"if_remote\""));

    git_config_identity(&sidecar_root);
    exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Bootstrap sidecar",
        ])
        .assert()
        .success();

    let status_output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "status",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status = json_result(&status_output);
    assert_eq!(status["linked"], true);
    assert_eq!(status["discovery"], JsonValue::Null);
    assert!(status["next_actions"].as_array().is_some_and(Vec::is_empty));
}

#[test]
fn sidecar_discovery_owner_template_flow_requires_acceptance_and_surfaces_status_action() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let sidecar_root = temp.path().join("owner-sidecars");
    let remote = seeded_sidecar_remote(temp.path(), "owner-template-sidecars", &[]);
    let remote_url = "https://github.com/wycats/owner-template-sidecars.git";
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[defaults]
root = "~/.exo/sidecars"
remote_template = "git@github.com:{{owner}}/{{repo}}-default.git"
auto_push = "never"

[owners."wycats"]
root = {:?}
remote_template = {:?}
auto_push = "always"
"#,
            sidecar_root.to_str().expect("sidecar root is utf-8"),
            remote_url
        ),
    )
    .expect("write registry");

    let discover_output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let discover = json_result(&discover_output);
    assert_eq!(discover["ok"], true);
    assert_eq!(discover["match"]["kind"], "owner-template");
    assert_eq!(discover["confidence"], "medium");
    assert_eq!(discover["proposal"]["key"], "locald");
    assert_eq!(discover["proposal"]["remote"], remote_url);
    assert_eq!(discover["proposal"]["auto_push"], "always");
    assert_eq!(discover["proposal"]["requires_remote_acceptance"], true);

    let rejected_output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&rejected_output);
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("requires --accept-discovered-remote")),
        "{error:?}"
    );

    let mut bootstrap_cmd = exo_cmd(&repo, &home, &config_home);
    add_git_remote_rewrite(&mut bootstrap_cmd, remote_url, &remote);
    let bootstrap_output = bootstrap_cmd
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--accept-discovered-remote",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let bootstrap = json_result(&bootstrap_output);
    assert_eq!(bootstrap["sidecar_key"], "locald");
    assert_eq!(bootstrap["discovery"]["match"]["kind"], "owner-template");
    assert_eq!(bootstrap["discovery"]["proposal"]["auto_push"], "always");
    assert_eq!(
        bootstrap["sidecar_root"].as_str(),
        Some(sidecar_root.to_str().expect("sidecar root is utf-8"))
    );

    let policy = std::fs::read_to_string(config_home.join("exo/projects.toml"))
        .expect("read project policy");
    assert!(policy.contains("sidecar_key = \"locald\""));
    assert!(policy.contains("auto_push = \"always\""));

    git_config_identity(&sidecar_root);
    exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Bootstrap sidecar",
        ])
        .assert()
        .success();

    let status_output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "status",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status = json_result(&status_output);
    assert_eq!(status["discovery"], JsonValue::Null);
    assert!(status["next_actions"].as_array().is_some_and(Vec::is_empty));
}

#[test]
fn sidecar_discovery_status_bootstrap_remote_flow_disappears_after_remote_is_configured() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let sidecar_root = temp.path().join("sidecars");
    let remote = seeded_sidecar_remote(temp.path(), "status-flow-sidecars", &[]);
    let remote_url = "https://github.com/wycats/status-flow-sidecars.git";
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald-flow"
root = {:?}
remote = {:?}
auto_push = "if_remote"
"#,
            sidecar_root.to_str().expect("sidecar root is utf-8"),
            remote_url
        ),
    )
    .expect("write registry");

    let pre_bootstrap_status_output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "status",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let pre_bootstrap_status = json_result(&pre_bootstrap_status_output);
    assert_eq!(pre_bootstrap_status["linked"], false);
    assert_eq!(pre_bootstrap_status["discovery"], JsonValue::Null);
    assert!(
        pre_bootstrap_status["next_actions"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );

    let mut bootstrap_cmd = exo_cmd(&repo, &home, &config_home);
    add_git_remote_rewrite(&mut bootstrap_cmd, remote_url, &remote);
    let bootstrap_output = bootstrap_cmd
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let bootstrap = json_result(&bootstrap_output);
    assert_eq!(bootstrap["sidecar_key"], "locald-flow");
    assert_eq!(bootstrap["discovery"]["ok"], true);

    git_config_identity(&sidecar_root);
    exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Bootstrap sidecar",
        ])
        .assert()
        .success();

    let remote_less_status_output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "status",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let remote_less_status = json_result(&remote_less_status_output);
    assert_eq!(remote_less_status["linked"], true);
    assert_eq!(remote_less_status["discovery"], JsonValue::Null);
    assert!(
        remote_less_status["next_actions"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );

    let remote_output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format", "json", "sidecar", "repo", "remote", "--url", remote_url,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let remote_result = json_result(&remote_output);
    assert_eq!(remote_result["remote"], "origin");
    assert_eq!(remote_result["url"], remote_url);

    let configured_status_output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "status",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let configured_status = json_result(&configured_status_output);
    assert_eq!(configured_status["linked"], true);
    assert_eq!(configured_status["discovery"], JsonValue::Null);
    assert!(
        configured_status["next_actions"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
}

#[test]
fn sidecar_bootstrap_discover_requires_acceptance_for_template_remote() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    let remote = seeded_sidecar_remote(temp.path(), "accepted-template-sidecars", &[]);
    let remote_url = "https://github.com/wycats/accepted-template-sidecars.git";
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        format!(
            r#"version = 1

[defaults]
root = "~/.exo/sidecars"
remote_template = {:?}
"#,
            remote_url
        ),
    )
    .expect("write registry");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("requires --accept-discovered-remote")),
        "{error:?}"
    );
    assert!(!config_home.join("exo/projects.toml").exists());

    let mut cmd = exo_cmd(&repo, &home, &config_home);
    add_git_remote_rewrite(&mut cmd, remote_url, &remote);
    let output = cmd
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--discover",
            "--accept-discovered-remote",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["sidecar_key"], "locald");
    assert_eq!(result["discovery"]["match"]["kind"], "defaults");
    assert_eq!(result["discovery"]["proposal"]["remote"], remote_url);
}

#[test]
fn sidecar_status_reports_discovered_remote_when_sidecar_has_no_remote() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let registry = temp.path().join("sidecars.toml");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );
    std::fs::write(
        &registry,
        r#"version = 1

[projects."github.com/wycats/locald"]
key = "locald"
root = "~/.exo/sidecars"
remote = "git@github.com:wycats/locald-exosuit-state.git"
auto_push = "if_remote"
"#,
    )
    .expect("write registry");

    exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success();
    git_config_identity(&home.join("exo/sidecars"));
    exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Bootstrap sidecar",
        ])
        .assert()
        .success();

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "status",
            "--registry-file",
            registry.to_str().expect("registry path is utf-8"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.status");
    assert_eq!(result["linked"], true);
    assert_eq!(result["discovery"]["ok"], true);
    assert_eq!(result["discovery"]["match"]["kind"], "exact-project");
    assert_eq!(
        result["discovery"]["proposal"]["remote"],
        "git@github.com:wycats/locald-exosuit-state.git"
    );
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions.iter().any(|action| {
            action["command"]
                == "exo sidecar repo remote --url git@github.com:wycats/locald-exosuit-state.git"
        })
    }));
}

#[cfg(unix)]
#[test]
fn sidecar_status_fetches_authenticated_user_profile_registry_when_sidecar_has_no_remote() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let fake_path = fake_gh_path(
        temp.path(),
        r#"#!/bin/sh
set -eu
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "user" ] && [ "$3" = "--jq" ] && [ "$4" = ".login" ]; then
    echo "alice"
    exit 0
fi
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "-H" ] && [ "$4" = "repos/alice/alice/contents/.exosuit/sidecars.toml" ]; then
    cat <<'EOF'
version = 1

[defaults]
root = "~/.exo/sidecars"
auto_push = "if_remote"

[projects."github.com/wycats/locald"]
key = "locald"
remote = "git@github.com:wycats/locald-exosuit-state.git"
EOF
    exit 0
fi
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "users/wycats" ] && [ "$3" = "--jq" ] && [ "$4" = ".type" ]; then
    echo "User"
    exit 0
fi
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "-H" ] && [ "$4" = "repos/wycats/wycats/contents/.exosuit/sidecars.toml" ]; then
    echo "HTTP 404: Not Found" >&2
    exit 1
fi
echo "unexpected gh invocation: $*" >&2
exit 2
"#,
    );
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );

    exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success();
    git_config_identity(&home.join("exo/sidecars"));
    exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Bootstrap sidecar",
        ])
        .assert()
        .success();

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "status"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                fake_path.to_str().expect("fake PATH is utf-8"),
                std::env::var("PATH").expect("PATH is set")
            ),
        )
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.status");
    assert_eq!(result["linked"], true);
    assert_eq!(result["discovery"]["ok"], true);
    assert_eq!(
        result["discovery"]["identity"]["source"],
        "authenticated-user"
    );
    assert_eq!(result["discovery"]["identity"]["login"], "alice");
    assert_eq!(result["discovery"]["registry"]["source"], "github-profile");
    assert_eq!(
        result["discovery"]["registry"]["profile_repo"],
        "github.com/alice/alice"
    );
    assert_eq!(result["discovery"]["attempt_index"], 0);
    assert_eq!(result["discovery"]["checked"][0]["status"], "loaded-match");
    assert_eq!(
        result["discovery"]["match"]["key"],
        "github.com/wycats/locald"
    );
    assert_eq!(
        result["discovery"]["proposal"]["remote"],
        "git@github.com:wycats/locald-exosuit-state.git"
    );
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions.iter().any(|action| {
            action["command"]
                == "exo sidecar repo remote --url git@github.com:wycats/locald-exosuit-state.git"
        })
    }));
}

#[cfg(unix)]
#[test]
fn sidecar_status_human_renders_profile_discovered_remote_when_sidecar_has_no_remote() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let fake_path = fake_gh_path(
        temp.path(),
        r#"#!/bin/sh
set -eu
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "user" ] && [ "$3" = "--jq" ] && [ "$4" = ".login" ]; then
    echo "alice"
    exit 0
fi
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "-H" ] && [ "$4" = "repos/alice/alice/contents/.exosuit/sidecars.toml" ]; then
    cat <<'EOF'
version = 1

[projects."github.com/wycats/locald"]
key = "locald"
remote = "git@github.com:wycats/locald-exosuit-state.git"
EOF
    exit 0
fi
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "users/wycats" ] && [ "$3" = "--jq" ] && [ "$4" = ".type" ]; then
    echo "User"
    exit 0
fi
if [ "$#" -eq 4 ] && [ "$1" = "api" ] && [ "$2" = "-H" ] && [ "$4" = "repos/wycats/wycats/contents/.exosuit/sidecars.toml" ]; then
    echo "HTTP 404: Not Found" >&2
    exit 1
fi
echo "unexpected gh invocation: $*" >&2
exit 2
"#,
    );
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );

    exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success();
    git_config_identity(&home.join("exo/sidecars"));
    exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Bootstrap sidecar",
        ])
        .assert()
        .success();

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["sidecar", "status"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                fake_path.to_str().expect("fake PATH is utf-8"),
                std::env::var("PATH").expect("PATH is set")
            ),
        )
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(output).expect("human output is utf-8");

    assert!(human.contains("Sidecar linked:"), "{human}");
    assert!(
        human.contains("Discovery: github-profile:.exosuit/sidecars.toml"),
        "{human}"
    );
    assert!(
        human.contains("Discovery location: github.com/alice/alice:.exosuit/sidecars.toml"),
        "{human}"
    );
    assert!(
        human.contains("Source: authenticated-user alice"),
        "{human}"
    );
    assert!(
        human.contains("Remote: git@github.com:wycats/locald-exosuit-state.git"),
        "{human}"
    );
    assert!(
        human.contains(
            "→ exo sidecar repo remote --url git@github.com:wycats/locald-exosuit-state.git"
        ),
        "{human}"
    );
}

#[cfg(unix)]
#[test]
fn sidecar_status_does_not_fetch_profile_registry_when_sidecar_remote_exists() {
    let temp = short_tempdir();
    let repo = temp.path().join("locald");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let fake_path = fake_gh_path(
        temp.path(),
        r#"#!/bin/sh
set -eu
echo "unexpected gh invocation: $*" >&2
exit 2
"#,
    );
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/locald.git",
        ],
    );

    exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success();
    exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "remote",
            "--url",
            "git@github.com:wycats/locald-exosuit-state.git",
        ])
        .assert()
        .success();

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "status"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                fake_path.to_str().expect("fake PATH is utf-8"),
                std::env::var("PATH").expect("PATH is set")
            ),
        )
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["linked"], true);
    assert_eq!(result["discovery"], JsonValue::Null);
    assert!(result["next_actions"].as_array().is_some_and(Vec::is_empty));
}

#[test]
fn sidecar_bootstrap_rerun_preserves_existing_custom_binding() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("custom-sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "bootstrap",
            "--key",
            "custom-key",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ])
        .assert()
        .success();

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.bootstrap");
    assert_eq!(result["sidecar_key"], "custom-key");
    assert_eq!(
        result["sidecar_root"].as_str(),
        Some(sidecar_root.to_str().expect("sidecar root is utf-8"))
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "project", "resolve"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    assert_eq!(result["project"]["sidecar_key"], "custom-key");
    assert_eq!(
        result["project"]["sidecar_root"].as_str(),
        Some(sidecar_root.to_str().expect("sidecar root is utf-8"))
    );
    assert_eq!(git_status_porcelain(&repo), "");
}

#[test]
fn sidecar_bootstrap_seeds_existing_projection() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(repo.join("docs/agent-context")).expect("create repo projection");
    std::fs::write(
        repo.join("docs/agent-context/epochs.sql"),
        "-- Auto-generated by exo. Regenerate: exo status\nINSERT INTO epochs_data(text_id, title, slug, reviewed, sort_key) VALUES('seeded-epoch', 'Seeded Epoch', 'seeded-epoch', 0, '00000000000000000000');\n",
    )
    .expect("write repo projection");
    git_init(&repo);

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.bootstrap");
    assert_eq!(result["seeded_from_repo"], true);
    assert_eq!(result["db_created"], true);

    let output = exo_direct_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "epoch", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    let epochs = result["epochs"].as_array().expect("epochs array");
    assert!(epochs.iter().any(|epoch| epoch["title"] == "Seeded Epoch"));
}

#[test]
fn sidecar_bootstrap_no_git_skips_sidecar_git_repo() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap", "--no-git"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    let sidecar_root = home.join("exo/sidecars");
    let sidecar_key = result["sidecar_key"]
        .as_str()
        .expect("bootstrap returns sidecar_key")
        .to_string();
    assert_eq!(result["kind"], "sidecar.bootstrap");
    assert_eq!(result["ok"], true);
    assert_eq!(result["ready"], false);
    assert_eq!(result["git_initialized"], false);
    assert_eq!(result["repo_clean"], false);
    assert_eq!(result["has_remote"], false);
    assert_eq!(
        result["sync_issue"].as_str(),
        Some("sidecar git initialization skipped")
    );
    assert!(!sidecar_root.join(".git").exists());
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions
            .iter()
            .any(|action| action["command"] == "exo sidecar bootstrap")
    }));

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "No Git Sidecar Write",
        ])
        .assert()
        .success();
    assert!(
        sidecar_root
            .join("projects")
            .join(&sidecar_key)
            .join("agent-context/epochs.sql")
            .exists()
    );
    assert!(
        !sidecar_root.join(".git").exists(),
        "sidecar writes must not create a fake git directory for --no-git sidecars"
    );
    assert_no_work_repo_daemon_runtime(&repo);
    assert_eq!(git_status_porcelain(&repo), "");
}

#[test]
fn sidecar_workflow_mutates_without_dirtying_external_repo() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "link",
            "--key",
            "external-test",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ])
        .assert()
        .success();

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "External Dogfood",
        ])
        .assert()
        .success();

    assert_eq!(git_status_porcelain(&repo), "");
    assert!(!repo.join(".exo").exists());
    assert!(!repo.join("docs").exists());
    assert!(
        sidecar_root
            .join("projects/external-test/agent-context/epochs.sql")
            .exists()
    );
    let epochs = std::fs::read_to_string(
        sidecar_root.join("projects/external-test/agent-context/epochs.sql"),
    )
    .expect("read sidecar epochs projection");
    assert!(epochs.contains("External Dogfood"));
}

#[test]
fn direct_write_mutation_auto_commits_sidecar_repo() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Auto Commit Me",
        ])
        .assert()
        .success();

    assert_eq!(git_status_porcelain(&sidecar_root), "");
    assert_eq!(git_status_porcelain(&repo), "");
    assert!(!repo.join("docs").exists());
    let log = git_output(&sidecar_root, &["log", "--oneline", "-1"]);
    assert!(log.contains("Auto-persist Exosuit sidecar state"));
    let committed = git_output(
        &sidecar_root,
        &[
            "show",
            "HEAD:projects/external-test/agent-context/epochs.sql",
        ],
    );
    assert!(committed.contains("Auto Commit Me"));
}

#[test]
fn direct_write_mutation_checkpoints_even_when_auto_commit_disabled() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    disable_sidecar_auto_commit(&config_home);

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Mandatory Local Checkpoint",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let envelope: JsonValue = serde_json::from_slice(&output).expect("command output is json");

    assert_eq!(envelope["status"], "ok");
    assert_eq!(
        envelope["post_write"]["sidecar_auto_persist"]["auto_commit"], false,
        "{envelope:?}"
    );
    assert_eq!(
        envelope["post_write"]["sidecar_auto_persist"]["committed"], true,
        "{envelope:?}"
    );
    assert_eq!(git_status_porcelain(&sidecar_root), "");
    let log = git_output(&sidecar_root, &["log", "--oneline", "-1"]);
    assert!(log.contains("Auto-persist Exosuit sidecar state"));
    let committed = git_output(
        &sidecar_root,
        &[
            "show",
            "HEAD:projects/external-test/agent-context/epochs.sql",
        ],
    );
    assert!(committed.contains("Mandatory Local Checkpoint"));
}

#[test]
fn sidecar_checkpoint_repairs_current_project_projection_debt() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct", "--format", "json", "epoch", "add", "--title", "Baseline",
        ])
        .assert()
        .success();
    insert_idea_for_sidecar_project(
        &sidecar_root,
        "external-test",
        "checkpoint-idea",
        "Checkpoint idea",
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "checkpoint",
            "--project",
            "external-test",
            "--message",
            "Checkpoint current project",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.checkpoint");
    assert_eq!(result["sidecar_key"], "external-test");
    assert_eq!(result["committed"], true);
    assert_eq!(git_status_porcelain(&sidecar_root), "");
    let log = git_output(&sidecar_root, &["log", "--oneline", "-1"]);
    assert!(log.contains("Checkpoint current project"));
    let committed = git_output(
        &sidecar_root,
        &[
            "show",
            "HEAD:projects/external-test/agent-context/ideas.sql",
        ],
    );
    assert!(committed.contains("checkpoint-idea"));
}

#[test]
fn sidecar_checkpoint_project_commits_only_named_project() {
    let temp = short_tempdir();
    let repo = temp.path().join("exo2-repo");
    let sandbox_repo = temp.path().join("sandboxd-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sandbox_repo).expect("create sandbox repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sandbox_repo);
    git_init(&sidecar_root);
    link_sidecar_with_key(&repo, &home, &config_home, &sidecar_root, "exo2");
    link_sidecar_with_key(
        &sandbox_repo,
        &home,
        &config_home,
        &sidecar_root,
        "sandboxd",
    );

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Exo2 Baseline",
        ])
        .assert()
        .success();
    exo_cmd(&sandbox_repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Sandbox Baseline",
        ])
        .assert()
        .success();
    insert_idea_for_sidecar_project(
        &sidecar_root,
        "sandboxd",
        "sandboxd-checkpoint",
        "Sandbox checkpoint",
    );
    let sandbox_ideas = sidecar_root.join("projects/sandboxd/agent-context/ideas.sql");
    std::fs::create_dir_all(sandbox_ideas.parent().expect("sandbox projection parent"))
        .expect("create sandbox projection dir");
    std::fs::write(
        &sandbox_ideas,
        "-- Auto-generated by exo. Regenerate: exo status\nINSERT INTO ideas_data(text_id,title,status,sort_key,slug) VALUES('sandboxd-checkpoint','Sandbox checkpoint','pending',0,NULL);\n",
    )
    .expect("write sandbox projection debt");

    let exo2_manual = sidecar_root.join("projects/exo2/agent-context/manual.sql");
    std::fs::write(&exo2_manual, "manual exo2 debt\n").expect("write exo2 manual file");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "checkpoint",
            "--project",
            "sandboxd",
            "--message",
            "Checkpoint sandboxd",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.checkpoint");
    assert_eq!(result["sidecar_key"], "sandboxd");
    assert_eq!(result["committed"], true);
    let status = git_status_porcelain(&sidecar_root);
    assert!(
        status.contains("projects/exo2/agent-context/manual.sql"),
        "{status}"
    );
    assert!(
        !status.contains("projects/sandboxd/"),
        "sandboxd project should be locally checkpointed: {status}"
    );
    let log = git_output(&sidecar_root, &["log", "--oneline", "-1"]);
    assert!(log.contains("Checkpoint sandboxd"));
    let committed = git_output(
        &sidecar_root,
        &["show", "HEAD:projects/sandboxd/agent-context/ideas.sql"],
    );
    assert!(committed.contains("sandboxd-checkpoint"));
    let exo2_committed = Command::new("git")
        .args([
            "cat-file",
            "-e",
            "HEAD:projects/exo2/agent-context/manual.sql",
        ])
        .current_dir(&sidecar_root)
        .output()
        .expect("run git cat-file");
    assert!(
        !exo2_committed.status.success(),
        "checkpoint --project sandboxd must not commit exo2 files"
    );
}

#[test]
fn sidecar_checkpoint_project_commits_existing_projection_without_foreign_db() {
    let temp = short_tempdir();
    let repo = temp.path().join("exo2-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar_with_key(&repo, &home, &config_home, &sidecar_root, "exo2");

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Exo2 Baseline",
        ])
        .assert()
        .success();

    let sandbox_inbox = sidecar_root.join("projects/sandboxd/agent-context/inbox.sql");
    std::fs::create_dir_all(sandbox_inbox.parent().expect("sandbox parent"))
        .expect("create sandbox projection dir");
    std::fs::write(&sandbox_inbox, "manual sandbox inbox debt\n")
        .expect("write sandbox projection debt");
    let sandbox_db = sidecar_root.join("projects/sandboxd/cache/exo.db");
    assert!(!sandbox_db.exists());

    let _guard = DaemonPathGuard::new(&repo);
    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "checkpoint",
            "--project",
            "sandboxd",
            "--message",
            "Checkpoint sandbox projection",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.checkpoint");
    assert_eq!(result["sidecar_key"], "sandboxd");
    assert_eq!(result["committed"], true);
    assert!(
        !sandbox_db.exists(),
        "checkpoint must not synthesize a foreign DB"
    );
    assert_eq!(git_status_porcelain(&sidecar_root), "");
    let committed = git_output(
        &sidecar_root,
        &["show", "HEAD:projects/sandboxd/agent-context/inbox.sql"],
    );
    assert_eq!(committed, "manual sandbox inbox debt\n");

    let marker_dir = sidecar_root.join(".git/exo-write-owners");
    let markers = std::fs::read_dir(&marker_dir)
        .expect("read ownership marker dir")
        .map(|entry| {
            let path = entry.expect("marker entry").path();
            let contents = std::fs::read_to_string(&path).expect("read ownership marker");
            serde_json::from_str::<JsonValue>(&contents).expect("marker json")
        })
        .collect::<Vec<_>>();
    assert!(
        markers.iter().any(|marker| marker["sidecar_key"] == "exo2"),
        "{markers:?}"
    );
    assert!(
        markers
            .iter()
            .all(|marker| marker["sidecar_key"] != "sandboxd"),
        "named foreign checkpoint must not acquire a sandboxd writer marker: {markers:?}"
    );
}

#[test]
fn sidecar_checkpoint_project_preserves_existing_projection_with_foreign_db() {
    let temp = short_tempdir();
    let repo = temp.path().join("exo2-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar_with_key(&repo, &home, &config_home, &sidecar_root, "exo2");

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Exo2 Baseline",
        ])
        .assert()
        .success();

    let sandbox_inbox = sidecar_root.join("projects/sandboxd/agent-context/inbox.sql");
    std::fs::create_dir_all(sandbox_inbox.parent().expect("sandbox parent"))
        .expect("create sandbox projection dir");
    std::fs::write(&sandbox_inbox, "manual sandbox inbox debt\n")
        .expect("write sandbox projection debt");
    let sandbox_db = sidecar_root.join("projects/sandboxd/cache/exo.db");
    std::fs::create_dir_all(sandbox_db.parent().expect("sandbox cache parent"))
        .expect("create sandbox cache dir");
    std::fs::copy(sidecar_root.join("projects/exo2/cache/exo.db"), &sandbox_db)
        .expect("copy foreign cache db");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "checkpoint",
            "--project",
            "sandboxd",
            "--message",
            "Checkpoint sandbox projection",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.checkpoint");
    assert_eq!(result["sidecar_key"], "sandboxd");
    assert_eq!(result["committed"], true);
    assert_eq!(git_status_porcelain(&sidecar_root), "");
    let committed = git_output(
        &sidecar_root,
        &["show", "HEAD:projects/sandboxd/agent-context/inbox.sql"],
    );
    assert_eq!(committed, "manual sandbox inbox debt\n");
}

#[test]
fn sidecar_checkpoint_project_rejects_escaping_project_key() {
    let temp = short_tempdir();
    let repo = temp.path().join("exo2-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let external = temp.path().join("outside-project");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&external).expect("create external target");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar_with_key(&repo, &home, &config_home, &sidecar_root, "exo2");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "checkpoint",
            "--project",
            external.to_str().expect("external path is utf-8"),
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("safe relative project key")),
        "{error:?}"
    );
    assert!(
        !external.join("agent-context").exists(),
        "invalid checkpoint key must not write projection outside the sidecar projects tree"
    );
}

#[test]
fn sidecar_checkpoint_project_respects_active_foreign_owner() {
    let temp = short_tempdir();
    let repo = temp.path().join("exo2-repo");
    let sandbox_workspace = temp.path().join("sandboxd-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sandbox_workspace).expect("create sandbox workspace");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar_with_key(&repo, &home, &config_home, &sidecar_root, "exo2");

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Exo2 Baseline",
        ])
        .assert()
        .success();

    let sandbox_inbox = sidecar_root.join("projects/sandboxd/agent-context/inbox.sql");
    std::fs::create_dir_all(sandbox_inbox.parent().expect("sandbox parent"))
        .expect("create sandbox projection dir");
    std::fs::write(&sandbox_inbox, "sandbox inbox debt\n").expect("write sandbox inbox");
    write_sidecar_write_owner_marker(
        &sidecar_root,
        "sandboxd",
        std::process::id(),
        &sandbox_workspace,
        None,
        None,
    );
    let before = git_output(&sidecar_root, &["rev-parse", "HEAD"]);

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "checkpoint",
            "--project",
            "sandboxd",
            "--message",
            "Checkpoint sandbox projection",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("another active runtime")),
        "{error:?}"
    );
    assert_eq!(git_output(&sidecar_root, &["rev-parse", "HEAD"]), before);
    assert!(
        git_status_porcelain(&sidecar_root).contains("projects/sandboxd/agent-context/inbox.sql"),
        "foreign projection debt should remain uncommitted"
    );
}

#[test]
fn sidecar_checkpoint_commits_previously_tracked_runtime_cleanup() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct", "--format", "json", "epoch", "add", "--title", "Baseline",
        ])
        .assert()
        .success();

    let runtime_file = sidecar_root.join("projects/external-test/runtime/legacy.pid");
    std::fs::create_dir_all(runtime_file.parent().expect("runtime parent"))
        .expect("create runtime dir");
    std::fs::write(&runtime_file, "12345\n").expect("write tracked runtime file");
    git_success(
        &sidecar_root,
        &["add", "-f", "projects/external-test/runtime/legacy.pid"],
    );
    git_success(&sidecar_root, &["commit", "-m", "Track legacy runtime"]);

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "checkpoint",
            "--message",
            "Drop tracked runtime",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["committed"], true, "{result:?}");
    assert_eq!(
        git_output(
            &sidecar_root,
            &["ls-files", "projects/external-test/runtime/legacy.pid"],
        ),
        ""
    );
    assert_eq!(git_status_porcelain(&sidecar_root), "");
    let log = git_output(&sidecar_root, &["log", "--oneline", "-1"]);
    assert!(log.contains("Drop tracked runtime"));
}

#[test]
fn sidecar_repo_commit_does_not_stage_foreign_project_deletions() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Current Project State",
        ])
        .assert()
        .success();

    let locald_path = sidecar_root.join("projects/locald/agent-context/epochs.sql");
    std::fs::create_dir_all(locald_path.parent().unwrap()).expect("create locald projection dir");
    std::fs::write(
        &locald_path,
        "-- Auto-generated by exo. Regenerate: exo status\nINSERT INTO epochs_data(text_id,title,status,sort_key,slug) VALUES('locald-epoch','Locald','pending',0,NULL);\n",
    )
    .expect("write locald projection");
    git_success(&sidecar_root, &["add", "projects/locald"]);
    git_success(&sidecar_root, &["commit", "-m", "Add locald sidecar state"]);
    let before = git_output(&sidecar_root, &["rev-parse", "HEAD"]);

    std::fs::remove_dir_all(sidecar_root.join("projects/locald")).expect("remove locald locally");
    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Commit current sidecar state",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(output).expect("human output is utf-8");
    assert!(
        human.contains("No owned sidecar changes to commit"),
        "{human}"
    );
    assert!(human.contains("unowned sidecar change"), "{human}");

    let after = git_output(&sidecar_root, &["rev-parse", "HEAD"]);
    assert_eq!(
        after, before,
        "foreign-only deletion must not create a commit"
    );
    assert!(
        git_status_porcelain(&sidecar_root).contains("projects/locald/agent-context/epochs.sql"),
        "foreign deletion should remain visible but unstaged"
    );
    let locald = git_output(
        &sidecar_root,
        &["show", "HEAD:projects/locald/agent-context/epochs.sql"],
    );
    assert!(locald.contains("Locald"));
}

#[test]
fn sidecar_repo_commit_rejects_pre_staged_foreign_project_changes() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Initial Project State",
        ])
        .assert()
        .success();
    disable_sidecar_auto_commit(&config_home);
    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Owned Pending State",
        ])
        .assert()
        .success();
    let owned_path = sidecar_root.join("projects/external-test/agent-context/manual.sql");
    std::fs::write(&owned_path, "manual\n").expect("write owned pending sidecar file");

    let locald_path = sidecar_root.join("projects/locald/agent-context/epochs.sql");
    std::fs::create_dir_all(locald_path.parent().unwrap()).expect("create locald projection dir");
    std::fs::write(
        &locald_path,
        "-- Auto-generated by exo. Regenerate: exo status\nINSERT INTO epochs_data(text_id,title,status,sort_key,slug) VALUES('locald-epoch','Locald','pending',0,NULL);\n",
    )
    .expect("write locald projection");
    git_success(&sidecar_root, &["add", "projects/locald"]);

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Commit owned state",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);
    assert!(
        error["message"].as_str().is_some_and(|message| {
            message.contains("staged changes outside the current project subtree")
                && message.contains("projects/locald/agent-context/epochs.sql")
        }),
        "{error:?}"
    );
    let log = git_output(&sidecar_root, &["log", "--oneline", "-1"]);
    assert!(log.contains("Auto-persist Exosuit sidecar state"));
    assert!(!log.contains("Commit owned state"));
}

#[test]
fn sidecar_repo_commit_rejects_pre_staged_foreign_project_changes_with_no_owned_changes() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Initial Project State",
        ])
        .assert()
        .success();

    let locald_path = sidecar_root.join("projects/locald/agent-context/epochs.sql");
    std::fs::create_dir_all(locald_path.parent().unwrap()).expect("create locald projection dir");
    std::fs::write(
        &locald_path,
        "-- Auto-generated by exo. Regenerate: exo status\nINSERT INTO epochs_data(text_id,title,status,sort_key,slug) VALUES('locald-epoch','Locald','pending',0,NULL);\n",
    )
    .expect("write locald projection");
    git_success(&sidecar_root, &["add", "projects/locald"]);

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "No owned state",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);
    assert!(
        error["message"].as_str().is_some_and(|message| {
            message.contains("staged changes outside the current project subtree")
                && message.contains("projects/locald/agent-context/epochs.sql")
        }),
        "{error:?}"
    );
    let log = git_output(&sidecar_root, &["log", "--oneline", "-1"]);
    assert!(log.contains("Auto-persist Exosuit sidecar state"));
    assert!(!log.contains("No owned state"));
}

#[test]
fn sidecar_repo_commit_rejects_cross_project_rename_out_of_owned_subtree() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Initial Project State",
        ])
        .assert()
        .success();

    let destination = sidecar_root.join("projects/locald/agent-context/epochs.sql");
    std::fs::create_dir_all(destination.parent().unwrap()).expect("create locald projection dir");
    git_success(
        &sidecar_root,
        &[
            "mv",
            "projects/external-test/agent-context/epochs.sql",
            "projects/locald/agent-context/epochs.sql",
        ],
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Commit cross-project rename",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);
    assert!(
        error["message"].as_str().is_some_and(|message| {
            message.contains("staged changes outside the current project subtree")
                && message.contains("projects/locald/agent-context/epochs.sql")
        }),
        "{error:?}"
    );
    let log = git_output(&sidecar_root, &["log", "--oneline", "-1"]);
    assert!(log.contains("Auto-persist Exosuit sidecar state"));
    assert!(!log.contains("Commit cross-project rename"));
}

#[test]
fn sidecar_repo_commit_installs_runtime_gitignore_before_staging_owned_paths() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    disable_sidecar_auto_commit(&config_home);

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Manual Commit State",
        ])
        .assert()
        .success();
    assert!(
        sidecar_root
            .join("projects/external-test/cache/exo.db")
            .exists()
    );

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Commit manual sidecar state",
        ])
        .assert()
        .success();

    assert_eq!(git_status_porcelain(&sidecar_root), "");
    assert_eq!(
        git_output(
            &sidecar_root,
            &["ls-files", "projects/external-test/cache/exo.db"],
        ),
        ""
    );
    let gitignore = git_output(&sidecar_root, &["show", "HEAD:.gitignore"]);
    assert!(gitignore.contains("projects/*/cache/"));
    assert!(gitignore.contains("projects/*/runtime/"));
    let committed = git_output(
        &sidecar_root,
        &[
            "show",
            "HEAD:projects/external-test/agent-context/epochs.sql",
        ],
    );
    assert!(committed.contains("Manual Commit State"));
}

#[test]
fn sidecar_repo_commit_stages_current_project_deletions() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Current Project State",
        ])
        .assert()
        .success();

    let stale_path = sidecar_root.join("projects/external-test/agent-context/stale.sql");
    std::fs::write(&stale_path, "stale\n").expect("write stale current-project file");
    git_success(
        &sidecar_root,
        &["add", "projects/external-test/agent-context/stale.sql"],
    );
    git_success(
        &sidecar_root,
        &["commit", "-m", "Add stale current project file"],
    );
    std::fs::remove_file(&stale_path).expect("delete stale current-project file");

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Drop stale current project file",
        ])
        .assert()
        .success();

    let output = Command::new("git")
        .args([
            "cat-file",
            "-e",
            "HEAD:projects/external-test/agent-context/stale.sql",
        ])
        .current_dir(&sidecar_root)
        .output()
        .expect("run git cat-file");
    assert!(
        !output.status.success(),
        "current project deletion should be committed"
    );
    assert_eq!(git_status_porcelain(&sidecar_root), "");
}

#[test]
fn auto_persist_from_stale_checkout_preserves_foreign_project_on_remote() {
    let temp = short_tempdir();
    let repo = temp.path().join("sandboxd-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let remote = temp.path().join("sidecars.git");
    let seeder = temp.path().join("seeder");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&remote).expect("create bare remote dir");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&remote);

    std::fs::write(sidecar_root.join("README.md"), "sidecar\n").expect("write initial file");
    git_success(&sidecar_root, &["add", "README.md"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial sidecar root"]);
    git_success(&sidecar_root, &["branch", "-M", "main"]);
    git_success(
        &sidecar_root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git_success(&sidecar_root, &["push", "-u", "origin", "main"]);

    git_success(
        temp.path(),
        &["clone", remote.to_str().unwrap(), seeder.to_str().unwrap()],
    );
    git_config_identity(&seeder);
    git_success(&seeder, &["checkout", "-B", "main", "origin/main"]);
    let locald_path = seeder.join("projects/locald/agent-context/epochs.sql");
    std::fs::create_dir_all(locald_path.parent().unwrap()).expect("create locald projection dir");
    std::fs::write(
        &locald_path,
        "-- Auto-generated by exo. Regenerate: exo status\nINSERT INTO epochs_data(text_id,title,status,sort_key,slug) VALUES('locald-epoch','Locald','pending',0,NULL);\n",
    )
    .expect("write locald projection");
    git_success(&seeder, &["add", "projects/locald"]);
    git_success(&seeder, &["commit", "-m", "Add locald sidecar state"]);
    git_success(&seeder, &["push", "origin", "HEAD:main"]);

    assert!(
        !sidecar_root.join("projects/locald").exists(),
        "writer checkout should be stale and missing locald"
    );
    link_sidecar_with_key(&repo, &home, &config_home, &sidecar_root, "sandboxd");
    set_sidecar_auto_push(&config_home, "always");

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Sandboxd State",
        ])
        .assert()
        .success();

    let locald = git_output(
        &remote,
        &["show", "main:projects/locald/agent-context/epochs.sql"],
    );
    assert!(locald.contains("Locald"));
    let sandboxd = git_output(
        &remote,
        &["show", "main:projects/sandboxd/agent-context/epochs.sql"],
    );
    assert!(sandboxd.contains("Sandboxd State"));
}

#[test]
fn auto_persist_auto_push_blocks_foreign_checkpoint_debt_before_fetch_merge() {
    let temp = short_tempdir();
    let repo = temp.path().join("sandboxd-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let remote = temp.path().join("sidecars.git");
    let seeder = temp.path().join("seeder");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&remote).expect("create bare remote dir");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&remote);

    std::fs::write(sidecar_root.join("README.md"), "sidecar\n").expect("write initial file");
    git_success(&sidecar_root, &["add", "README.md"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial sidecar root"]);
    git_success(&sidecar_root, &["branch", "-M", "main"]);
    git_success(
        &sidecar_root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git_success(&sidecar_root, &["push", "-u", "origin", "main"]);

    git_success(
        temp.path(),
        &["clone", remote.to_str().unwrap(), seeder.to_str().unwrap()],
    );
    git_config_identity(&seeder);
    git_success(&seeder, &["checkout", "-B", "main", "origin/main"]);
    std::fs::write(seeder.join("remote.md"), "remote sidecar update\n")
        .expect("write remote update");
    git_success(&seeder, &["add", "remote.md"]);
    git_success(&seeder, &["commit", "-m", "Remote sidecar update"]);
    git_success(&seeder, &["push", "origin", "HEAD:main"]);

    link_sidecar_with_key(&repo, &home, &config_home, &sidecar_root, "sandboxd");
    set_sidecar_auto_push(&config_home, "always");
    let locald_debt = sidecar_root.join("projects/locald/agent-context/tasks.sql");
    std::fs::create_dir_all(locald_debt.parent().expect("locald parent"))
        .expect("create locald projection dir");
    std::fs::write(&locald_debt, "locald checkpoint debt\n").expect("write locald debt");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Sandboxd State",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let envelope: JsonValue = serde_json::from_slice(&output).expect("command output is json");

    assert_eq!(
        envelope["post_write"]["sidecar_auto_persist"]["ok"], false,
        "{envelope:?}"
    );
    assert!(
        envelope["post_write"]["sidecar_auto_persist"]["issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("foreign or cross-project checkpoint debt")),
        "{envelope:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&locald_debt).expect("read locald debt"),
        "locald checkpoint debt\n"
    );
    let sandboxd = git_output(
        &sidecar_root,
        &["show", "HEAD:projects/sandboxd/agent-context/epochs.sql"],
    );
    assert!(sandboxd.contains("Sandboxd State"));
    let remote_sandboxd = Command::new("git")
        .args([
            "cat-file",
            "-e",
            "main:projects/sandboxd/agent-context/epochs.sql",
        ])
        .current_dir(&remote)
        .output()
        .expect("run git cat-file");
    assert!(
        !remote_sandboxd.status.success(),
        "auto-push should stop before publishing current project"
    );
}

#[test]
fn sidecar_repo_status_human_omits_reclaimable_dead_owner() {
    let fixture = basic_sidecar_fixture();
    let dead_pid = exited_child_pid();
    write_sidecar_write_owner_marker(
        &fixture.sidecar_root,
        "external-test",
        dead_pid,
        &fixture.repo,
        None,
        None,
    );

    let json_output = exo_cmd(&fixture.repo, &fixture.home, &fixture.config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&json_output);
    assert_eq!(result["ownership"]["ok"], true, "{result:?}");
    assert_eq!(result["ownership"]["state"], "stale", "{result:?}");
    assert_eq!(
        result["ownership"]["owner_pid"].as_u64(),
        Some(u64::from(dead_pid)),
        "{result:?}"
    );
    assert!(
        result["ownership"]["issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("dead runtime")),
        "{result:?}"
    );

    let human_output = exo_cmd(&fixture.repo, &fixture.home, &fixture.config_home)
        .args(["--direct", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(human_output).expect("human output is utf-8");
    assert!(human.contains("Sidecar repo clean"), "{human}");
    assert!(!human.contains("Ownership:"), "{human}");
    assert!(!human.contains("dead runtime"), "{human}");
}

#[test]
fn sidecar_repo_status_human_reports_live_owner_block() {
    let fixture = basic_sidecar_fixture();
    let other_workspace = fixture._temp.path().join("other-workspace");
    let owner_pid = std::process::id();
    write_sidecar_write_owner_marker(
        &fixture.sidecar_root,
        "external-test",
        owner_pid,
        &other_workspace,
        None,
        None,
    );

    let human_output = exo_cmd(&fixture.repo, &fixture.home, &fixture.config_home)
        .args(["--direct", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(human_output).expect("human output is utf-8");
    assert!(
        human.contains("Ownership blocked by active runtime"),
        "{human}"
    );
    assert!(human.contains(&owner_pid.to_string()), "{human}");
    assert!(human.contains("another active runtime"), "{human}");
}

#[test]
fn daemon_sidecar_repo_status_human_reports_live_owner_block() {
    let fixture = basic_sidecar_fixture();
    let other_workspace = fixture._temp.path().join("other-workspace");
    let owner_pid = std::process::id();
    write_sidecar_write_owner_marker(
        &fixture.sidecar_root,
        "external-test",
        owner_pid,
        &other_workspace,
        None,
        None,
    );
    let _guard = DaemonPathGuard::new(&fixture.repo);

    let human_output = exo_cmd(&fixture.repo, &fixture.home, &fixture.config_home)
        .args(["sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(human_output).expect("human output is utf-8");
    assert!(human.contains("Sidecar repo clean"), "{human}");
    assert!(
        human.contains("Ownership blocked by active runtime"),
        "{human}"
    );
    assert!(human.contains(&owner_pid.to_string()), "{human}");
    assert!(
        human.contains(other_workspace.to_string_lossy().as_ref()),
        "{human}"
    );
}

#[test]
fn daemon_sidecar_repo_status_human_reports_invalid_owner_marker() {
    let fixture = basic_sidecar_fixture();
    let marker_path = sidecar_write_owner_marker_path(&fixture.sidecar_root, "external-test");
    std::fs::create_dir_all(marker_path.parent().expect("marker parent"))
        .expect("create marker parent");
    std::fs::write(&marker_path, "{not valid json").expect("write invalid marker");
    let _guard = DaemonPathGuard::new(&fixture.repo);

    let human_output = exo_cmd(&fixture.repo, &fixture.home, &fixture.config_home)
        .args(["sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(human_output).expect("human output is utf-8");
    assert!(human.contains("Ownership marker invalid:"), "{human}");
    assert!(!human.contains("Ownership blocked"), "{human}");
}

#[test]
fn daemon_sidecar_repo_status_human_reports_foreign_checkpoint_debt() {
    let fixture = basic_sidecar_fixture();
    let foreign_projection = fixture
        .sidecar_root
        .join("projects/other-project/agent-context/tasks.sql");
    std::fs::create_dir_all(
        foreign_projection
            .parent()
            .expect("foreign projection parent"),
    )
    .expect("create foreign projection parent");
    std::fs::write(&foreign_projection, "foreign projection\n").expect("write foreign projection");
    let _guard = DaemonPathGuard::new(&fixture.repo);

    let human_output = exo_cmd(&fixture.repo, &fixture.home, &fixture.config_home)
        .args(["sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(human_output).expect("human output is utf-8");
    assert!(human.contains("Foreign checkpoint debt:"), "{human}");
    assert!(human.contains("other-project: 1 file(s)"), "{human}");
    assert!(
        human.contains("exo sidecar checkpoint --project other-project"),
        "{human}"
    );
}

#[test]
fn sidecar_repo_status_human_reports_invalid_owner_marker() {
    let fixture = basic_sidecar_fixture();
    let marker_path = sidecar_write_owner_marker_path(&fixture.sidecar_root, "external-test");
    std::fs::create_dir_all(marker_path.parent().expect("marker parent"))
        .expect("create marker parent");
    std::fs::write(&marker_path, "{not valid json").expect("write invalid marker");

    let human_output = exo_cmd(&fixture.repo, &fixture.home, &fixture.config_home)
        .args(["--direct", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(human_output).expect("human output is utf-8");
    assert!(human.contains("Ownership marker invalid:"), "{human}");
    assert!(!human.contains("Ownership: blocked"), "{human}");
}

#[test]
fn auto_persist_blocks_live_incompatible_sidecar_write_owner() {
    let fixture = basic_sidecar_fixture();
    let repo = &fixture.repo;
    let home = &fixture.home;
    let config_home = &fixture.config_home;
    let sidecar_root = &fixture.sidecar_root;

    let other_workspace = fixture._temp.path().join("other-workspace");
    write_sidecar_write_owner_marker(
        sidecar_root,
        "external-test",
        std::process::id(),
        &other_workspace,
        Some("owner-binary-hash".to_string()),
        None,
    );
    let before = git_output(sidecar_root, &["rev-parse", "HEAD"]);

    let output = exo_cmd(repo, home, config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Blocked Owner State",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("another active runtime")),
        "{error:?}"
    );
    assert!(
        error["message"].as_str().is_some_and(|message| {
            message.contains(other_workspace.to_string_lossy().as_ref())
                && message.contains(&std::process::id().to_string())
                && message.contains("sidecar key: external-test")
                && message.contains("exo-write-owners/external-test-")
                && message.contains("owner db:")
                && message.contains("owner-binary-hash")
        }),
        "{error:?}"
    );
    let ownership_details = &error["details"]["details"];
    assert_eq!(ownership_details["kind"], "sidecar.write_ownership");
    assert_eq!(
        ownership_details["owner_pid"],
        u64::from(std::process::id())
    );
    assert_eq!(
        ownership_details["owner_workspace_root"].as_str(),
        Some(other_workspace.to_string_lossy().as_ref())
    );

    let after = git_output(sidecar_root, &["rev-parse", "HEAD"]);
    assert_eq!(
        after, before,
        "blocked owner must not create a sidecar commit"
    );
    assert!(
        !git_status_porcelain(sidecar_root).contains("agent-context/"),
        "blocked write must not update SQL projection files"
    );

    let output = exo_cmd(repo, home, config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    assert_eq!(result["ownership"]["ok"], false);
    assert_eq!(result["ownership"]["state"], "blocked");
    assert!(
        result["ownership"]["issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("another active runtime")),
        "{result:?}"
    );
}

#[cfg(unix)]
#[test]
fn auto_persist_blocks_live_same_workspace_owner_from_other_process() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    std::fs::write(sidecar_root.join("README.md"), "sidecar\n").expect("write readme");
    git_success(&sidecar_root, &["add", "README.md"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial sidecar"]);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    write_sidecar_write_owner_marker(
        &sidecar_root,
        "external-test",
        std::process::id(),
        &repo,
        Some(exo_binary_blake3()),
        Some(process_start_identity(std::process::id())),
    );
    let before = git_output(&sidecar_root, &["rev-parse", "HEAD"]);

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Blocked Same Workspace Owner",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("another active runtime")),
        "{error:?}"
    );

    let after = git_output(&sidecar_root, &["rev-parse", "HEAD"]);
    assert_eq!(
        after, before,
        "live owner with same project and binary but different pid must not be overwritten"
    );
    assert!(
        !git_status_porcelain(&sidecar_root).contains("agent-context/"),
        "blocked write must not update SQL projection files"
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    assert_eq!(result["ownership"]["ok"], false);
    assert_eq!(result["ownership"]["state"], "blocked");
    assert!(
        result["ownership"]["issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("another active runtime")),
        "{result:?}"
    );
}

#[cfg(unix)]
#[test]
fn auto_persist_blocks_old_different_binary_exo_owner_without_heartbeat() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    std::fs::write(sidecar_root.join("README.md"), "sidecar\n").expect("write readme");
    git_success(&sidecar_root, &["add", "README.md"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial sidecar"]);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    let canonical_repo = repo.canonicalize().expect("canonical repo");

    let (
        mut old_runtime,
        old_runtime_pid,
        old_runtime_hash,
        old_runtime_start_id,
        old_runtime_path,
    ) = spawned_stale_exo_pid_hash_start_id_and_path(
        &temp.path().join("fixtures"),
        &repo,
        &home,
        &config_home,
    );
    write_sidecar_write_owner_marker_with_options(
        &sidecar_root,
        "external-test",
        old_runtime_pid,
        &canonical_repo,
        Some(&old_runtime_path),
        Some(old_runtime_hash),
        Some(old_runtime_start_id),
        1,
    );
    let status_output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status = json_result(&status_output);
    assert_eq!(status["ownership"]["ok"], false);
    assert_eq!(status["ownership"]["state"], "blocked");
    assert!(
        status["ownership"]["issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("different executable identity")),
        "{status:?}"
    );
    let before = git_output(&sidecar_root, &["rev-parse", "HEAD"]);

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Reclaimed Orphaned Owner State",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("different executable identity")),
        "{error:?}"
    );

    assert!(
        process_alive(old_runtime_pid),
        "old different-binary Exo owner must not be terminated without heartbeat proof"
    );
    let after = git_output(&sidecar_root, &["rev-parse", "HEAD"]);
    assert_eq!(
        after, before,
        "blocked different-binary owner must not create a sidecar commit"
    );
    assert!(
        !git_status_porcelain(&sidecar_root).contains("agent-context/"),
        "blocked write must not update SQL projection files"
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    assert_eq!(result["ownership"]["ok"], false);
    assert_eq!(result["ownership"]["state"], "blocked");
    assert_eq!(
        result["ownership"]["owner_pid"].as_u64(),
        Some(u64::from(old_runtime_pid))
    );
    cleanup_process(old_runtime_pid);
    let _ = old_runtime.wait();
}

#[cfg(unix)]
#[test]
fn live_stale_owner_without_start_identity_is_not_reclaimable() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    std::fs::write(sidecar_root.join("README.md"), "sidecar\n").expect("write readme");
    git_success(&sidecar_root, &["add", "README.md"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial sidecar"]);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    let canonical_repo = repo.canonicalize().expect("canonical repo");

    let (old_runtime_pid, old_runtime_hash, _) = detached_sleep_pid_hash_and_start_id();
    write_sidecar_write_owner_marker(
        &sidecar_root,
        "external-test",
        old_runtime_pid,
        &canonical_repo,
        Some(old_runtime_hash),
        None,
    );

    let status_output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status = json_result(&status_output);
    assert_eq!(status["ownership"]["ok"], false);
    assert_eq!(status["ownership"]["state"], "blocked");
    assert!(
        status["ownership"]["issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("stale Exo runtime")),
        "{status:?}"
    );

    let commit_output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Should Not Reclaim Older Marker",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&commit_output);
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("stale Exo runtime")),
        "{error:?}"
    );
    assert!(
        process_alive(old_runtime_pid),
        "older marker without start identity must not be terminated"
    );
    cleanup_process(old_runtime_pid);
}

#[cfg(unix)]
#[test]
fn auto_persist_blocks_non_exo_owner_even_when_marker_matches_process() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    std::fs::write(sidecar_root.join("README.md"), "sidecar\n").expect("write readme");
    git_success(&sidecar_root, &["add", "README.md"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial sidecar"]);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    let canonical_repo = repo.canonicalize().expect("canonical repo");

    let (owner_pid, owner_hash, owner_start_id) = detached_sleep_pid_hash_and_start_id();
    write_sidecar_write_owner_marker(
        &sidecar_root,
        "external-test",
        owner_pid,
        &canonical_repo,
        Some(owner_hash),
        Some(owner_start_id),
    );
    let before = git_output(&sidecar_root, &["rev-parse", "HEAD"]);

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Blocked Non Exo Owner State",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("different executable identity")),
        "{error:?}"
    );

    assert!(
        process_alive(owner_pid),
        "matching non-Exo owner process must not be terminated"
    );
    let after = git_output(&sidecar_root, &["rev-parse", "HEAD"]);
    assert_eq!(
        after, before,
        "blocked non-Exo owner must not create a sidecar commit"
    );
    assert!(
        !git_status_porcelain(&sidecar_root).contains("agent-context/"),
        "blocked write must not update SQL projection files"
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    assert_eq!(result["ownership"]["ok"], false);
    assert_eq!(result["ownership"]["state"], "blocked");
    assert_eq!(
        result["ownership"]["owner_pid"].as_u64(),
        Some(u64::from(owner_pid))
    );
    cleanup_process(owner_pid);
}

#[cfg(unix)]
#[test]
fn auto_persist_blocks_fresh_different_binary_exo_owner() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    std::fs::write(sidecar_root.join("README.md"), "sidecar\n").expect("write readme");
    git_success(&sidecar_root, &["add", "README.md"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial sidecar"]);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    let canonical_repo = repo.canonicalize().expect("canonical repo");

    let (mut owner, owner_pid, owner_hash, owner_start_id, owner_path) =
        spawned_stale_exo_pid_hash_start_id_and_path(
            &temp.path().join("fixtures"),
            &repo,
            &home,
            &config_home,
        );
    let refreshed_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis();
    write_sidecar_write_owner_marker_with_options(
        &sidecar_root,
        "external-test",
        owner_pid,
        &canonical_repo,
        Some(&owner_path),
        Some(owner_hash),
        Some(owner_start_id),
        refreshed_at_ms,
    );
    let before = git_output(&sidecar_root, &["rev-parse", "HEAD"]);

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Blocked Fresh Exo Owner State",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("different executable identity")),
        "{error:?}"
    );

    assert!(
        process_alive(owner_pid),
        "fresh different-binary Exo owner must not be terminated"
    );
    let after = git_output(&sidecar_root, &["rev-parse", "HEAD"]);
    assert_eq!(
        after, before,
        "fresh different-binary owner must not create a sidecar commit"
    );
    assert!(
        !git_status_porcelain(&sidecar_root).contains("agent-context/"),
        "blocked write must not update SQL projection files"
    );
    cleanup_process(owner_pid);
    let _ = owner.wait();
}

#[cfg(unix)]
#[test]
fn auto_persist_refuses_orphaned_owner_when_live_process_identity_mismatches_marker() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    std::fs::write(sidecar_root.join("README.md"), "sidecar\n").expect("write readme");
    git_success(&sidecar_root, &["add", "README.md"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial sidecar"]);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    let canonical_repo = repo.canonicalize().expect("canonical repo");

    let (owner_pid, _owner_hash, owner_start_id) = detached_sleep_pid_hash_and_start_id();
    write_sidecar_write_owner_marker(
        &sidecar_root,
        "external-test",
        owner_pid,
        &canonical_repo,
        Some("tampered-owner-hash".to_string()),
        Some(owner_start_id),
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Blocked Tampered Owner State",
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("different executable identity")),
        "{error:?}"
    );

    assert!(
        process_alive(owner_pid),
        "mismatched live owner process must not be terminated"
    );
    assert!(
        !git_status_porcelain(&sidecar_root).contains("agent-context/"),
        "blocked write must not update SQL projection files"
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    assert_eq!(result["ownership"]["ok"], false);
    assert_eq!(result["ownership"]["state"], "blocked");
    assert_eq!(
        result["ownership"]["owner_pid"].as_u64(),
        Some(u64::from(owner_pid))
    );
    cleanup_process(owner_pid);
}

#[cfg(unix)]
#[test]
fn sidecar_status_does_not_classify_zero_pid_owner_as_reclaimable() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    std::fs::write(sidecar_root.join("README.md"), "sidecar\n").expect("write readme");
    git_success(&sidecar_root, &["add", "README.md"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial sidecar"]);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    let canonical_repo = repo.canonicalize().expect("canonical repo");

    write_sidecar_write_owner_marker(
        &sidecar_root,
        "external-test",
        0,
        &canonical_repo,
        Some("stale-exo-runtime-binary".to_string()),
        None,
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    assert_eq!(result["ownership"]["ok"], false);
    assert_eq!(result["ownership"]["state"], "blocked");
    assert!(
        result["ownership"]["issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("stale Exo runtime")),
        "{result:?}"
    );
}

#[test]
fn auto_persist_reclaims_dead_sidecar_write_owner() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    std::fs::write(sidecar_root.join("README.md"), "sidecar\n").expect("write readme");
    git_success(&sidecar_root, &["add", "README.md"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial sidecar"]);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    let dead_pid = exited_child_pid();
    write_sidecar_write_owner_marker(&sidecar_root, "external-test", dead_pid, &repo, None, None);
    let before = git_output(&sidecar_root, &["rev-parse", "HEAD"]);

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Reclaimed Owner State",
        ])
        .assert()
        .success();

    let after = git_output(&sidecar_root, &["rev-parse", "HEAD"]);
    assert_ne!(
        after, before,
        "dead owner should be reclaimed and committed"
    );
    assert_eq!(git_status_porcelain(&sidecar_root), "");

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    assert_eq!(result["ownership"]["ok"], true);
    assert_eq!(result["ownership"]["state"], "stale");
    assert_ne!(
        result["ownership"]["owner_pid"].as_u64(),
        Some(dead_pid as u64)
    );
}

#[test]
fn auto_persist_untracks_accidentally_tracked_sidecar_cache() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Initial Sidecar State",
        ])
        .assert()
        .success();

    git_success(
        &sidecar_root,
        &["add", "-f", "projects/external-test/cache/exo.db"],
    );
    git_success(&sidecar_root, &["commit", "-m", "Accidentally track cache"]);
    assert_eq!(
        git_output(
            &sidecar_root,
            &["ls-files", "projects/external-test/cache/exo.db"],
        ),
        "projects/external-test/cache/exo.db\n"
    );

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Repair Tracked Cache",
        ])
        .assert()
        .success();

    assert_eq!(git_status_porcelain(&sidecar_root), "");
    assert_eq!(
        git_output(
            &sidecar_root,
            &["ls-files", "projects/external-test/cache/exo.db"],
        ),
        ""
    );
    let committed = git_output(
        &sidecar_root,
        &[
            "show",
            "HEAD:projects/external-test/agent-context/epochs.sql",
        ],
    );
    assert!(committed.contains("Repair Tracked Cache"));
}

#[test]
fn pure_command_does_not_create_sidecar_commit() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "epoch", "list"])
        .assert()
        .success();

    let output = Command::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .current_dir(&sidecar_root)
        .output()
        .expect("run git rev-parse");
    assert!(!output.status.success());
}

#[test]
fn sidecar_repo_commit_does_not_recursively_auto_commit() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    std::fs::write(sidecar_root.join("manual.md"), "manual\n").expect("write sidecar file");

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Manual sidecar commit",
        ])
        .assert()
        .success();

    let count = git_output(&sidecar_root, &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "1");
    let log = git_output(&sidecar_root, &["log", "--oneline", "-1"]);
    assert!(log.contains("Manual sidecar commit"));
}

#[test]
fn direct_auto_persist_push_failure_does_not_fail_mutation() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/nope.git",
        ],
    );
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    set_sidecar_auto_push(&config_home, "always");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Push Failure Still Writes",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let envelope: serde_json::Value =
        serde_json::from_slice(&output).expect("command output is json");
    assert_eq!(
        envelope["post_write"]["sidecar_auto_persist"]["ok"], false,
        "{envelope:?}"
    );
    assert!(
        envelope["post_write"]["sidecar_auto_persist"]["issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("git fetch")),
        "{envelope:?}"
    );

    assert_eq!(git_status_porcelain(&sidecar_root), "");
    let committed = git_output(
        &sidecar_root,
        &[
            "show",
            "HEAD:projects/external-test/agent-context/epochs.sql",
        ],
    );
    assert!(committed.contains("Push Failure Still Writes"));

    let output = exo_direct_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status = json_result(&output);
    let repair_actions = status["steering"]["repair_actions"]
        .as_array()
        .expect("repair actions array");
    assert_eq!(status["sidecar_sync"]["ok"], false, "{status:?}");
    assert!(repair_actions.iter().any(|action| {
        action["command"] == "exo sidecar repo push"
            && action["rationale"]
                .as_str()
                .is_some_and(|rationale| rationale.contains("not been pushed"))
    }));
}

#[test]
fn direct_auto_persist_push_uses_configured_remote_name() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let remote = temp.path().join("sidecars.git");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    std::fs::create_dir_all(&remote).expect("create bare remote dir");
    git_success(&remote, &["init", "--bare"]);
    git_success(
        &sidecar_root,
        &["remote", "add", "backup", remote.to_str().unwrap()],
    );
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Push To Backup",
        ])
        .assert()
        .success();

    assert_eq!(git_status_porcelain(&sidecar_root), "");
    let remote_ref = git_output(&remote, &["rev-parse", "refs/heads/main"]);
    assert!(!remote_ref.trim().is_empty());

    let output = exo_direct_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status = json_result(&output);
    assert_eq!(status["sidecar_sync"]["ok"], true, "{status:?}");
    assert_eq!(status["sidecar_sync"]["remote"], "backup");
}

#[test]
fn status_surfaces_sidecar_root_that_is_not_git_repo() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("not-a-git-sidecar");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    let output = exo_direct_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status = json_result(&output);
    assert_eq!(status["sidecar_sync"]["ok"], false, "{status:?}");
    assert!(
        status["sidecar_sync"]["issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("not a git repository")),
        "{status:?}"
    );
}

#[test]
fn sidecar_rehydrates_local_db_from_projection_without_repo_metadata() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "link",
            "--key",
            "external-test",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ])
        .assert()
        .success();

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "External Dogfood",
        ])
        .assert()
        .success();

    let db_path = sidecar_root.join("projects/external-test/cache/exo.db");
    assert!(db_path.exists());
    assert!(!home.join(".exo/sidecars/external-test").exists());
    std::fs::remove_file(&db_path).expect("remove sidecar project db");

    let output = exo_direct_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "epoch", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    let epochs = result["epochs"].as_array().expect("epochs array");

    assert!(db_path.exists());
    assert!(epochs.iter().any(|epoch| {
        epoch["title"] == "External Dogfood" || epoch["label"] == "External Dogfood"
    }));
    assert_eq!(git_status_porcelain(&repo), "");
    assert!(!repo.join("exosuit.toml").exists());
    assert!(!repo.join("docs").exists());
}

#[test]
fn sidecar_unlink_removes_only_local_binding() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "link",
            "--key",
            "external-test",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ])
        .assert()
        .success();

    let db_path = sidecar_root.join("projects/external-test/cache/exo.db");
    let manifest_path = sidecar_root.join("projects/external-test/sidecar.toml");
    assert!(db_path.exists());
    assert!(!home.join(".exo/sidecars/external-test").exists());
    assert!(manifest_path.exists());

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "unlink"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.unlink");
    assert_eq!(result["removed"], true);
    assert!(db_path.exists());
    assert!(manifest_path.exists());

    let status_output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status = json_result(&status_output);
    assert_eq!(status["linked"], false);
}

#[test]
fn sidecar_repo_status_reports_git_state() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct", "--format", "json", "epoch", "add", "--title", "Baseline",
        ])
        .assert()
        .success();
    std::fs::write(sidecar_root.join("loose.txt"), "dirty").expect("write sidecar file");
    let diagnostics_path = temp.path().join("sidecar-repo-status-daemon.ndjson");
    let _guard = DaemonPathGuard::new(&repo);

    let output = exo_cmd(&repo, &home, &config_home)
        .env("EXO_DAEMON_DIAGNOSTICS", "1")
        .env("EXO_DAEMON_DIAG_PATH", &diagnostics_path)
        .args(["--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    let events = wait_for_diagnostics_event(&diagnostics_path, "request.invoke_end");
    assert_has_daemon_operation(&events, "sidecar", "repo");
    assert_no_work_repo_daemon_runtime(&repo);
    assert_eq!(result["kind"], "sidecar.repo.status");
    assert_eq!(
        result["sidecar_root"].as_str(),
        Some(sidecar_root.to_str().expect("sidecar root is utf-8"))
    );
    assert_eq!(result["branch"], "main");
    assert_eq!(result["clean"], true);
    assert_eq!(result["repo_clean"], false);
    assert_eq!(result["has_remote"], false);
    let files = result["files"].as_array().expect("files array");
    assert!(files.iter().any(|file| file["path"] == "loose.txt"));
    let project_files = result["project_files"]
        .as_array()
        .expect("project files array");
    assert!(project_files.is_empty(), "{result:?}");
    let foreign_debt = result["foreign_checkpoint_debt"]
        .as_array()
        .expect("foreign debt array");
    assert!(foreign_debt.is_empty(), "{result:?}");

    let human_output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(human_output).expect("human output is utf-8");
    assert!(human.contains("Sidecar repo dirty"), "{human}");
    assert!(
        human.contains("Issue: sidecar repo has uncommitted changes"),
        "{human}"
    );
    assert!(human.contains("exo sidecar repo status"), "{human}");
}

#[test]
fn sidecar_repo_status_uses_configured_sidecar_key_boundary() {
    let temp = short_tempdir();
    let repo = temp.path().join("exo2-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar_with_key(&repo, &home, &config_home, &sidecar_root, "exo2");

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Exo2 Baseline",
        ])
        .assert()
        .success();

    let nested_project = sidecar_root.join("projects/org/repo");
    std::fs::create_dir_all(nested_project.join("agent-context"))
        .expect("create nested sidecar project");
    std::fs::write(
        nested_project.join("sidecar.toml"),
        "[sidecar]\nkey = \"org/repo\"\nproject_id = \"org-repo\"\n",
    )
    .expect("write nested sidecar manifest");
    std::fs::write(
        nested_project.join("agent-context/tasks.sql"),
        "nested project checkpoint debt\n",
    )
    .expect("write nested project debt");
    std::fs::create_dir_all(nested_project.join("cache")).expect("create nested cache");
    std::fs::write(nested_project.join("cache/exo.db"), "ignored cache\n")
        .expect("write nested cache");
    std::fs::create_dir_all(nested_project.join("runtime")).expect("create nested runtime");
    std::fs::write(nested_project.join("runtime/daemon.pid"), "123\n")
        .expect("write nested runtime");

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    let foreign_debt = result["foreign_checkpoint_debt"]
        .as_array()
        .expect("foreign debt array");

    assert_eq!(foreign_debt.len(), 1, "{result:?}");
    assert_eq!(foreign_debt[0]["project"], "org/repo", "{result:?}");
    let debt_files = foreign_debt[0]["files"].as_array().expect("debt files");
    assert!(
        debt_files.iter().all(
            |file| !file["path"].as_str().unwrap_or("").contains("/cache/")
                && !file["path"].as_str().unwrap_or("").contains("/runtime/")
        ),
        "{result:?}"
    );
    assert!(
        foreign_debt[0]["next_actions"]
            .as_array()
            .expect("debt actions")
            .iter()
            .any(|action| action["command"] == "exo sidecar checkpoint --project org/repo"),
        "{result:?}"
    );
}

#[test]
fn sidecar_repo_status_preserves_deleted_nested_key_from_tracked_manifest() {
    let temp = short_tempdir();
    let repo = temp.path().join("exo2-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_config_identity(&sidecar_root);
    link_sidecar_with_key(&repo, &home, &config_home, &sidecar_root, "exo2");

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Exo2 Baseline",
        ])
        .assert()
        .success();

    let nested_project = sidecar_root.join("projects/org/repo");
    std::fs::create_dir_all(nested_project.join("agent-context"))
        .expect("create nested sidecar project");
    std::fs::write(
        nested_project.join("sidecar.toml"),
        "[sidecar]\nkey = \"org/repo\"\nproject_id = \"org-repo\"\n",
    )
    .expect("write nested sidecar manifest");
    std::fs::write(
        nested_project.join("agent-context/tasks.sql"),
        "nested project checkpoint\n",
    )
    .expect("write nested project projection");
    git_success(&sidecar_root, &["add", "projects/org/repo"]);
    git_success(
        &sidecar_root,
        &["commit", "-m", "Add nested sidecar project"],
    );

    std::fs::remove_dir_all(&nested_project).expect("delete nested project");

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    let foreign_debt = result["foreign_checkpoint_debt"]
        .as_array()
        .expect("foreign debt array");

    assert_eq!(foreign_debt.len(), 1, "{result:?}");
    assert_eq!(foreign_debt[0]["project"], "org/repo", "{result:?}");
    assert!(
        foreign_debt[0]["next_actions"]
            .as_array()
            .expect("debt actions")
            .iter()
            .any(|action| action["command"] == "exo sidecar checkpoint --project org/repo"),
        "{result:?}"
    );
}

#[test]
fn sidecar_repo_status_scopes_foreign_checkpoint_debt() {
    let temp = short_tempdir();
    let repo = temp.path().join("exo2-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let remote = temp.path().join("sidecars.git");
    let seeder = temp.path().join("remote-seeder");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&remote).expect("create remote root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&remote);
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path is utf-8"),
        ],
    );
    link_sidecar_with_key(&repo, &home, &config_home, &sidecar_root, "exo2");

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Exo2 Baseline",
        ])
        .assert()
        .success();
    git_success(&sidecar_root, &["push", "-u", "origin", "main"]);

    let sandbox_inbox = sidecar_root.join("projects/sandboxd/agent-context/inbox.sql");
    let sandbox_tasks = sidecar_root.join("projects/sandboxd/agent-context/tasks.sql");
    std::fs::create_dir_all(sandbox_inbox.parent().expect("sandbox parent"))
        .expect("create sandbox projection dir");
    std::fs::write(&sandbox_inbox, "sandbox inbox debt\n").expect("write sandbox inbox");
    std::fs::write(&sandbox_tasks, "sandbox tasks debt\n").expect("write sandbox tasks");
    let sandbox_old = sidecar_root.join("projects/sandboxd/agent-context/old.sql");
    let sandbox_new = sidecar_root.join("projects/sandboxd/agent-context/new.sql");
    std::fs::write(&sandbox_old, "sandbox rename source\n").expect("write sandbox old file");
    git_success(
        &sidecar_root,
        &["add", "projects/sandboxd/agent-context/old.sql"],
    );
    git_success(
        &sidecar_root,
        &["commit", "-m", "Add sandbox old projection"],
    );
    git_success(&sidecar_root, &["push", "origin", "main"]);
    std::fs::create_dir_all(sandbox_new.parent().expect("sandbox parent"))
        .expect("create sandbox projection dir");
    git_success(
        &sidecar_root,
        &[
            "mv",
            "projects/sandboxd/agent-context/old.sql",
            "projects/sandboxd/agent-context/new.sql",
        ],
    );
    git_success(
        temp.path(),
        &["clone", remote.to_str().unwrap(), seeder.to_str().unwrap()],
    );
    git_config_identity(&seeder);
    git_success(&seeder, &["checkout", "-B", "main", "origin/main"]);
    std::fs::write(seeder.join("remote.md"), "remote sidecar update\n")
        .expect("write remote update");
    git_success(&seeder, &["add", "remote.md"]);
    git_success(&seeder, &["commit", "-m", "Remote sidecar update"]);
    git_success(&seeder, &["push", "origin", "main"]);
    git_success(&sidecar_root, &["fetch", "origin"]);

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["ok"], true, "{result:?}");
    assert_eq!(result["clean"], true, "{result:?}");
    assert_eq!(result["repo_clean"], false, "{result:?}");
    assert_eq!(result["behind"], 1, "{result:?}");
    assert_eq!(
        result["issue_kind"].as_str(),
        Some("foreign_checkpoint_debt"),
        "{result:?}"
    );
    assert_eq!(result["syncable"], false, "{result:?}");
    assert_eq!(
        result["project_files"]
            .as_array()
            .expect("project files")
            .len(),
        0,
        "{result:?}"
    );
    let foreign_debt = result["foreign_checkpoint_debt"]
        .as_array()
        .expect("foreign debt array");
    assert_eq!(foreign_debt.len(), 1, "{result:?}");
    assert_eq!(foreign_debt[0]["project"], "sandboxd");
    let files = foreign_debt[0]["files"].as_array().expect("debt files");
    assert_eq!(files.len(), 3, "{foreign_debt:?}");
    assert!(
        files
            .iter()
            .any(|file| file["path"] == "projects/sandboxd/agent-context/inbox.sql")
    );
    assert!(
        files
            .iter()
            .any(|file| file["path"] == "projects/sandboxd/agent-context/tasks.sql")
    );
    assert!(
        files.iter().any(|file| file["path"].as_str().is_some_and(
            |path| path == "projects/sandboxd/agent-context/old.sql -> projects/sandboxd/agent-context/new.sql"
        )),
        "{files:?}"
    );
    assert!(
        foreign_debt[0]["next_actions"]
            .as_array()
            .expect("debt actions")
            .iter()
            .any(|action| action["command"] == "exo sidecar checkpoint --project sandboxd")
    );
    assert!(
        result["next_actions"]
            .as_array()
            .expect("next actions")
            .iter()
            .any(|action| action["command"] == "exo sidecar checkpoint --project sandboxd")
    );
    assert!(
        result["next_actions"]
            .as_array()
            .expect("next actions")
            .iter()
            .all(|action| action["command"] != "exo sidecar repo sync"),
        "{result:?}"
    );
    let sync_output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "sync"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let sync = json_result(&sync_output);
    assert_eq!(sync["kind"], "sidecar.repo.sync");
    assert_eq!(
        sync["issue_kind"].as_str(),
        Some("foreign_checkpoint_debt"),
        "{sync:?}"
    );

    let status_output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status = json_result(&status_output);
    assert_eq!(status["sidecar_sync"]["ok"], false, "{status:?}");
    assert_eq!(status["sidecar_sync"]["clean"], true, "{status:?}");
    assert_eq!(status["sidecar_sync"]["repo_clean"], false, "{status:?}");
    assert_eq!(
        status["sidecar_sync"]["issue_kind"].as_str(),
        Some("foreign_checkpoint_debt"),
        "{status:?}"
    );
    assert_eq!(status["sidecar_sync"]["syncable"], false, "{status:?}");
    assert!(
        status["steering"]["repair_actions"]
            .as_array()
            .expect("repair actions")
            .iter()
            .any(|action| action["command"] == "exo sidecar checkpoint --project sandboxd")
    );

    let sidecar_status_output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let sidecar_status = json_result(&sidecar_status_output);
    assert_eq!(
        sidecar_status["sidecar_repo"]["ok"], false,
        "{sidecar_status:?}"
    );
    assert_eq!(
        sidecar_status["sidecar_repo"]["issue_kind"].as_str(),
        Some("foreign_checkpoint_debt"),
        "{sidecar_status:?}"
    );
    assert!(
        sidecar_status["next_actions"]
            .as_array()
            .expect("sidecar status next actions")
            .iter()
            .any(|action| action["command"] == "exo sidecar checkpoint --project sandboxd"),
        "{sidecar_status:?}"
    );
    let sidecar_human_output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "sidecar", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let sidecar_human = String::from_utf8(sidecar_human_output).expect("human output is utf-8");
    assert!(
        sidecar_human.contains(
            "Sidecar repo issue: sidecar repo has foreign or cross-project checkpoint debt"
        ),
        "{sidecar_human}"
    );
    assert!(
        sidecar_human.contains("exo sidecar checkpoint --project sandboxd"),
        "{sidecar_human}"
    );

    let human_output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(human_output).expect("human output is utf-8");
    assert!(human.contains("Foreign checkpoint debt:"), "{human}");
    assert!(
        human.contains("exo sidecar checkpoint --project sandboxd"),
        "{human}"
    );
}

#[test]
fn sidecar_repo_status_reports_cross_project_rename_without_checkpoint_suggestion() {
    let temp = short_tempdir();
    let repo = temp.path().join("exo2-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar_with_key(&repo, &home, &config_home, &sidecar_root, "exo2");

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Exo2 Baseline",
        ])
        .assert()
        .success();

    let sandbox_old = sidecar_root.join("projects/sandboxd/agent-context/old.sql");
    std::fs::create_dir_all(sandbox_old.parent().expect("sandbox parent"))
        .expect("create sandbox projection dir");
    std::fs::write(&sandbox_old, "sandbox projection\n").expect("write sandbox old file");
    git_success(
        &sidecar_root,
        &["add", "projects/sandboxd/agent-context/old.sql"],
    );
    git_success(&sidecar_root, &["commit", "-m", "Add sandbox projection"]);
    std::fs::create_dir_all(sidecar_root.join("projects/locald/agent-context"))
        .expect("create locald projection dir");
    git_success(
        &sidecar_root,
        &[
            "mv",
            "projects/sandboxd/agent-context/old.sql",
            "projects/locald/agent-context/new.sql",
        ],
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(
        result["issue_kind"].as_str(),
        Some("foreign_checkpoint_debt"),
        "{result:?}"
    );
    assert_eq!(result["syncable"], false, "{result:?}");
    let foreign_debt = result["foreign_checkpoint_debt"]
        .as_array()
        .expect("foreign debt array");
    assert_eq!(foreign_debt.len(), 1, "{result:?}");
    assert_eq!(foreign_debt[0]["project"], "cross-project");
    assert_eq!(foreign_debt[0]["checkpointable"], false);
    assert_eq!(
        foreign_debt[0]["issue_kind"].as_str(),
        Some("cross_project_move")
    );
    let files = foreign_debt[0]["files"].as_array().expect("debt files");
    assert!(
        files.iter().any(|file| file["path"].as_str().is_some_and(
            |path| path == "projects/sandboxd/agent-context/old.sql -> projects/locald/agent-context/new.sql"
        )),
        "{files:?}"
    );
    let debt_actions = foreign_debt[0]["next_actions"]
        .as_array()
        .expect("debt actions");
    assert!(
        debt_actions
            .iter()
            .any(|action| action["command"] == "exo sidecar repo status"),
        "{debt_actions:?}"
    );
    assert!(
        debt_actions.iter().all(|action| !action["command"]
            .as_str()
            .is_some_and(|command| command.starts_with("exo sidecar checkpoint --project"))),
        "{debt_actions:?}"
    );
}

#[test]
fn sidecar_repo_status_reports_project_to_loose_rename_without_checkpoint_suggestion() {
    let temp = short_tempdir();
    let repo = temp.path().join("exo2-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar_with_key(&repo, &home, &config_home, &sidecar_root, "exo2");

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Exo2 Baseline",
        ])
        .assert()
        .success();

    let exo_old = sidecar_root.join("projects/exo2/agent-context/old.sql");
    std::fs::create_dir_all(exo_old.parent().expect("exo parent"))
        .expect("create exo projection dir");
    std::fs::write(&exo_old, "exo projection\n").expect("write exo old file");
    git_success(
        &sidecar_root,
        &["add", "projects/exo2/agent-context/old.sql"],
    );
    git_success(&sidecar_root, &["commit", "-m", "Add exo projection"]);
    std::fs::create_dir_all(sidecar_root.join("archive")).expect("create archive dir");
    git_success(
        &sidecar_root,
        &[
            "mv",
            "projects/exo2/agent-context/old.sql",
            "archive/old.sql",
        ],
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(
        result["issue_kind"].as_str(),
        Some("foreign_checkpoint_debt"),
        "{result:?}"
    );
    let foreign_debt = result["foreign_checkpoint_debt"]
        .as_array()
        .expect("foreign debt array");
    assert_eq!(foreign_debt.len(), 1, "{result:?}");
    assert_eq!(foreign_debt[0]["project"], "cross-project");
    assert_eq!(foreign_debt[0]["checkpointable"], false);
    assert_eq!(
        foreign_debt[0]["issue_kind"].as_str(),
        Some("cross_project_move")
    );
    let debt_actions = foreign_debt[0]["next_actions"]
        .as_array()
        .expect("debt actions");
    assert!(
        debt_actions
            .iter()
            .any(|action| action["command"] == "exo sidecar repo status"),
        "{debt_actions:?}"
    );
    assert!(
        debt_actions.iter().all(|action| !action["command"]
            .as_str()
            .is_some_and(|command| command.starts_with("exo sidecar checkpoint --project"))),
        "{debt_actions:?}"
    );
}

#[test]
fn sidecar_checkpoint_project_commits_deletion_only_foreign_debt() {
    let temp = short_tempdir();
    let repo = temp.path().join("exo2-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar_with_key(&repo, &home, &config_home, &sidecar_root, "exo2");

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Exo2 Baseline",
        ])
        .assert()
        .success();

    let sandbox_inbox = sidecar_root.join("projects/sandboxd/agent-context/inbox.sql");
    std::fs::create_dir_all(sandbox_inbox.parent().expect("sandbox parent"))
        .expect("create sandbox projection dir");
    std::fs::write(&sandbox_inbox, "sandbox inbox\n").expect("write sandbox projection");
    git_success(
        &sidecar_root,
        &["add", "projects/sandboxd/agent-context/inbox.sql"],
    );
    git_success(&sidecar_root, &["commit", "-m", "Add sandbox projection"]);
    std::fs::remove_dir_all(sidecar_root.join("projects/sandboxd"))
        .expect("remove sandbox project");

    let status_output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status = json_result(&status_output);
    assert!(
        status["foreign_checkpoint_debt"][0]["next_actions"]
            .as_array()
            .expect("debt actions")
            .iter()
            .any(|action| action["command"] == "exo sidecar checkpoint --project sandboxd"),
        "{status:?}"
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "checkpoint",
            "--project",
            "sandboxd",
            "--message",
            "Checkpoint sandbox deletion",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["sidecar_key"], "sandboxd");
    assert_eq!(result["committed"], true);
    assert_eq!(git_status_porcelain(&sidecar_root), "");
    let deleted = Command::new("git")
        .args([
            "cat-file",
            "-e",
            "HEAD:projects/sandboxd/agent-context/inbox.sql",
        ])
        .current_dir(&sidecar_root)
        .output()
        .expect("run git cat-file");
    assert!(
        !deleted.status.success(),
        "sandbox projection should be deleted"
    );
}

#[test]
fn sidecar_checkpoint_project_commits_tracked_foreign_runtime_deletion() {
    let temp = short_tempdir();
    let repo = temp.path().join("exo2-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar_with_key(&repo, &home, &config_home, &sidecar_root, "exo2");

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "epoch",
            "add",
            "--title",
            "Exo2 Baseline",
        ])
        .assert()
        .success();

    let runtime_file = sidecar_root.join("projects/sandboxd/runtime/legacy.pid");
    std::fs::create_dir_all(runtime_file.parent().expect("runtime parent"))
        .expect("create sandbox runtime dir");
    std::fs::write(&runtime_file, "12345\n").expect("write sandbox runtime file");
    git_success(
        &sidecar_root,
        &["add", "-f", "projects/sandboxd/runtime/legacy.pid"],
    );
    git_success(&sidecar_root, &["commit", "-m", "Track sandbox runtime"]);
    std::fs::remove_dir_all(sidecar_root.join("projects/sandboxd"))
        .expect("remove sandbox project");

    let status_output = exo_cmd(&repo, &home, &config_home)
        .args(["--direct", "--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status = json_result(&status_output);
    let foreign_debt = status["foreign_checkpoint_debt"]
        .as_array()
        .expect("foreign debt array");
    assert_eq!(foreign_debt.len(), 1, "{status:?}");
    let files = foreign_debt[0]["files"].as_array().expect("debt files");
    assert!(
        files
            .iter()
            .any(|file| file["path"] == "projects/sandboxd/runtime/legacy.pid"),
        "{files:?}"
    );

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "checkpoint",
            "--project",
            "sandboxd",
            "--message",
            "Checkpoint sandbox runtime deletion",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["sidecar_key"], "sandboxd");
    assert_eq!(result["committed"], true);
    assert_eq!(git_status_porcelain(&sidecar_root), "");
    let deleted = Command::new("git")
        .args([
            "cat-file",
            "-e",
            "HEAD:projects/sandboxd/runtime/legacy.pid",
        ])
        .current_dir(&sidecar_root)
        .output()
        .expect("run git cat-file");
    assert!(
        !deleted.status.success(),
        "sandbox runtime file should be deleted"
    );
}

#[test]
fn sidecar_repo_commit_commits_sidecar_only_and_leaves_work_repo_clean() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    std::fs::write(sidecar_root.join("personal.md"), "sidecar note\n").expect("write sidecar note");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Commit sidecar note",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.repo.commit");
    assert_eq!(result["committed"], true);
    assert_eq!(git_status_porcelain(&sidecar_root), "?? personal.md\n");
    assert_eq!(git_status_porcelain(&repo), "");
    assert!(!repo.join("docs").exists());
    let log = git_output(&sidecar_root, &["log", "--oneline", "-1"]);
    assert!(log.contains("Commit sidecar note"));
}

#[test]
fn sidecar_repo_commit_without_direct_uses_daemon_and_keeps_work_repo_clean() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let diagnostics_path = temp.path().join("sidecar-repo-commit-daemon.ndjson");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    std::fs::write(sidecar_root.join("personal.md"), "sidecar note\n").expect("write sidecar note");
    let _guard = DaemonPathGuard::new(&repo);

    let output = exo_cmd(&repo, &home, &config_home)
        .env("EXO_DAEMON_DIAGNOSTICS", "1")
        .env("EXO_DAEMON_DIAG_PATH", &diagnostics_path)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Commit sidecar note through daemon",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    let events = wait_for_diagnostics_event(&diagnostics_path, "request.invoke_end");
    assert_has_daemon_operation(&events, "sidecar", "repo");
    assert_no_work_repo_daemon_runtime(&repo);
    assert_eq!(result["kind"], "sidecar.repo.commit");
    assert_eq!(result["committed"], true);
    assert_eq!(git_status_porcelain(&sidecar_root), "?? personal.md\n");
    assert_eq!(git_status_porcelain(&repo), "");
    assert!(!repo.join("docs").exists());
    let log = git_output(&sidecar_root, &["log", "--oneline", "-1"]);
    assert!(log.contains("Commit sidecar note through daemon"));
}

#[test]
fn sidecar_repo_commit_flushes_projection_before_committing() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    disable_sidecar_auto_commit(&config_home);

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct", "--format", "json", "epoch", "add", "--title", "Flush Me",
        ])
        .assert()
        .success();
    let projection = sidecar_root.join("projects/external-test/agent-context/epochs.sql");
    assert!(projection.exists());
    std::fs::remove_file(&projection).expect("remove stale projection before commit");

    let status_before = git_status_porcelain(&sidecar_root);
    assert!(status_before.contains("epochs.sql"), "{status_before}");

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--direct",
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Flush sidecar projection",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["committed"], false);
    assert_eq!(git_status_porcelain(&sidecar_root), "");
    let committed = git_output(
        &sidecar_root,
        &[
            "show",
            "HEAD:projects/external-test/agent-context/epochs.sql",
        ],
    );
    assert!(committed.contains("Flush Me"));
    assert_eq!(git_status_porcelain(&repo), "");
}

#[test]
fn sidecar_repo_push_uses_existing_remote() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let remote = temp.path().join("sidecars.git");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&remote).expect("create remote root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&remote);
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path is utf-8"),
        ],
    );
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    std::fs::write(sidecar_root.join("push.md"), "push me\n").expect("write push file");
    exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Pushable sidecar commit",
        ])
        .assert()
        .success();

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "repo", "push"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.repo.push");
    assert_eq!(result["remote"], "origin");
    assert_eq!(result["branch"], "main");
    assert_eq!(result["pushed"], true);
    let remote_ref = git_output(&remote, &["rev-parse", "refs/heads/main"]);
    assert!(!remote_ref.trim().is_empty());
    let upstream = git_output(&sidecar_root, &["rev-parse", "--abbrev-ref", "@{upstream}"]);
    assert_eq!(upstream.trim(), "origin/main");
}

#[test]
fn sidecar_bootstrap_suggests_pull_when_sidecar_repo_is_behind() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let remote = temp.path().join("sidecars.git");
    let other = temp.path().join("other-sidecar");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&remote).expect("create remote root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&remote);
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path is utf-8"),
        ],
    );
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    std::fs::write(
        sidecar_root.join(".gitignore"),
        "projects/*/cache/\nprojects/*/runtime/\n",
    )
    .expect("write sidecar gitignore");
    std::fs::write(sidecar_root.join("initial.md"), "initial\n").expect("write initial file");
    git_success(&sidecar_root, &["add", "-A"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial sidecar"]);
    git_success(&sidecar_root, &["push", "-u", "origin", "main"]);

    let remote_url = remote.to_str().expect("remote path is utf-8");
    git_success(temp.path(), &["clone", remote_url, other.to_str().unwrap()]);
    git_config_identity(&other);
    git_success(&other, &["checkout", "-B", "main", "origin/main"]);
    std::fs::write(other.join("remote.md"), "remote\n").expect("write remote file");
    git_success(&other, &["add", "-A"]);
    git_success(&other, &["commit", "-m", "Remote sidecar update"]);
    git_success(&other, &["push", "origin", "main"]);
    git_success(&sidecar_root, &["fetch", "origin"]);

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert!(
        result["sync_issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("behind its upstream")),
        "{result:?}"
    );
    assert_eq!(result["ok"], true);
    assert_eq!(result["ready"], false);
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions
            .iter()
            .any(|action| action["command"].as_str() == Some("exo sidecar repo sync"))
    }));
}

#[test]
fn sidecar_status_reports_unrelated_sidecar_history_recovery() {
    let fixture = unrelated_sidecar_fixture();

    let output = exo_direct_cmd(&fixture.repo, &fixture.home, &fixture.config_home)
        .args(["--format", "json", "sidecar", "status"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.status");
    assert_eq!(result["linked"], true);
    assert_eq!(result["ok"], false, "{result:?}");
    assert_eq!(result["sidecar_repo"]["ok"], false, "{result:?}");
    assert_eq!(
        result["sidecar_repo"]["issue_kind"].as_str(),
        Some("unrelated_history"),
        "{result:?}"
    );
    assert!(
        result["sidecar_repo"]["issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("unrelated history")),
        "{result:?}"
    );
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        !actions.is_empty()
            && actions.iter().all(|action| {
                action["command"]
                    .as_str()
                    .is_some_and(|command| command.starts_with("exo "))
            })
    }));
}

#[test]
fn sidecar_bootstrap_does_not_suggest_sync_for_unrelated_history() {
    let fixture = unrelated_sidecar_fixture();

    let output = exo_direct_cmd(&fixture.repo, &fixture.home, &fixture.config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.bootstrap");
    assert_eq!(result["ok"], true, "{result:?}");
    assert_eq!(result["ready"], false, "{result:?}");
    assert!(
        result["sync_issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("unrelated history")),
        "{result:?}"
    );
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        !actions
            .iter()
            .any(|action| action["command"].as_str() == Some("exo sidecar repo sync"))
    }));
}

#[test]
fn sidecar_repo_status_classifies_unrelated_history() {
    let fixture = unrelated_sidecar_fixture();

    let output = exo_direct_cmd(&fixture.repo, &fixture.home, &fixture.config_home)
        .args(["--format", "json", "sidecar", "repo", "status"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.repo.status");
    assert_eq!(result["clean"], true, "{result:?}");
    assert_eq!(result["ok"], false, "{result:?}");
    assert_eq!(result["syncable"], false, "{result:?}");
    assert_eq!(
        result["issue_kind"].as_str(),
        Some("unrelated_history"),
        "{result:?}"
    );
    assert!(
        result["issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("unrelated history")),
        "{result:?}"
    );
    assert!(result["next_actions"].as_array().is_some_and(|actions| {
        actions
            .iter()
            .any(|action| action["command"].as_str() == Some("exo sidecar repo status"))
    }));

    let human_output = exo_direct_cmd(&fixture.repo, &fixture.home, &fixture.config_home)
        .args(["sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let human = String::from_utf8(human_output).expect("human output is utf-8");
    assert!(
        human.contains("Issue: sidecar repo local branch and upstream have unrelated history"),
        "{human}"
    );
    assert!(human.contains("Next actions:"), "{human}");
    assert!(human.contains("exo sidecar repo status"), "{human}");
}

#[test]
fn sidecar_repo_status_keeps_unrelated_history_ahead_of_foreign_debt() {
    let fixture = unrelated_sidecar_fixture();
    let foreign_projection = fixture
        .sidecar_root
        .join("projects/sandboxd/agent-context/tasks.sql");
    std::fs::create_dir_all(
        foreign_projection
            .parent()
            .expect("foreign projection parent"),
    )
    .expect("create foreign projection parent");
    std::fs::write(&foreign_projection, "sandboxd checkpoint debt\n")
        .expect("write foreign projection debt");

    let output = exo_direct_cmd(&fixture.repo, &fixture.home, &fixture.config_home)
        .args(["--format", "json", "sidecar", "repo", "status"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.repo.status");
    assert_eq!(result["ok"], false, "{result:?}");
    assert_eq!(
        result["issue_kind"].as_str(),
        Some("unrelated_history"),
        "{result:?}"
    );
    assert_eq!(
        result["foreign_checkpoint_debt"]
            .as_array()
            .expect("foreign debt")
            .len(),
        1,
        "{result:?}"
    );
    assert!(
        result["next_actions"]
            .as_array()
            .expect("next actions")
            .iter()
            .any(|action| action["command"] == "exo sidecar repo status"),
        "{result:?}"
    );
    assert!(
        result["next_actions"]
            .as_array()
            .expect("next actions")
            .iter()
            .all(|action| action["command"] != "exo sidecar checkpoint --project sandboxd"),
        "{result:?}"
    );
}

#[test]
fn sidecar_repo_sync_reports_structured_unrelated_history() {
    let fixture = unrelated_sidecar_fixture();

    let output = exo_direct_cmd(&fixture.repo, &fixture.home, &fixture.config_home)
        .args(["--format", "json", "sidecar", "repo", "sync"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.repo.sync");
    assert_eq!(result["ok"], false, "{result:?}");
    assert_eq!(
        result["issue_kind"].as_str(),
        Some("unrelated_history"),
        "{result:?}"
    );
    assert!(
        result["issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("unrelated history")),
        "{result:?}"
    );
    assert!(
        !result["issue"]
            .as_str()
            .is_some_and(|issue| issue.contains("git merge-base failed")),
        "{result:?}"
    );
}

#[test]
fn status_steers_unrelated_sidecar_history_to_status_not_push() {
    let fixture = unrelated_sidecar_fixture();

    let output = exo_direct_cmd(&fixture.repo, &fixture.home, &fixture.config_home)
        .args(["--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);
    let repair_actions = result["steering"]["repair_actions"]
        .as_array()
        .expect("repair actions array");

    assert!(
        repair_actions
            .iter()
            .any(|action| { action["command"].as_str() == Some("exo sidecar repo status") })
    );
    assert!(
        !repair_actions
            .iter()
            .any(|action| { action["command"].as_str() == Some("exo sidecar repo push") })
    );
}

#[test]
fn sidecar_repo_sync_semantically_merges_distinct_ideas() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let remote = temp.path().join("sidecars.git");
    let other = temp.path().join("other-sidecar");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&remote).expect("create remote root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&remote);
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path is utf-8"),
        ],
    );
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Initial sidecar state",
        ])
        .assert()
        .success();
    git_success(&sidecar_root, &["push", "-u", "origin", "main"]);

    let remote_url = remote.to_str().expect("remote path is utf-8");
    git_success(temp.path(), &["clone", remote_url, other.to_str().unwrap()]);
    git_config_identity(&other);
    git_success(&other, &["checkout", "-B", "main", "origin/main"]);
    append_idea_sql(&other, "remote-idea", "Remote idea");
    git_success(&other, &["add", "-A"]);
    git_success(&other, &["commit", "-m", "Remote idea"]);
    git_success(&other, &["push", "origin", "main"]);

    insert_idea(&sidecar_root, "local-idea", "Local idea");

    let output = exo_direct_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "repo", "sync"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.repo.sync");
    assert_eq!(result["ok"], true, "{result:?}");
    assert_eq!(result["merged"], true, "{result:?}");
    assert_eq!(result["pushed"], true, "{result:?}");

    let local_ideas = std::fs::read_to_string(
        sidecar_root.join("projects/external-test/agent-context/ideas.sql"),
    )
    .expect("read local ideas");
    assert!(local_ideas.contains("local-idea"), "{local_ideas}");
    assert!(local_ideas.contains("remote-idea"), "{local_ideas}");

    let remote_ideas = git_output(
        &remote,
        &[
            "show",
            "refs/heads/main:projects/external-test/agent-context/ideas.sql",
        ],
    );
    assert!(remote_ideas.contains("local-idea"), "{remote_ideas}");
    assert!(remote_ideas.contains("remote-idea"), "{remote_ideas}");
    assert_eq!(git_status_porcelain(&sidecar_root), "");
}

#[test]
fn sidecar_repo_sync_merges_same_named_remote_branch_without_upstream() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let remote = temp.path().join("sidecars.git");
    let other = temp.path().join("other-sidecar");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&remote).expect("create remote root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&remote);
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path is utf-8"),
        ],
    );
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Initial sidecar state",
        ])
        .assert()
        .success();
    git_success(&sidecar_root, &["push", "origin", "main"]);

    let upstream = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "@{upstream}"])
        .current_dir(&sidecar_root)
        .output()
        .expect("check upstream");
    assert!(
        !upstream.status.success(),
        "test setup should leave local main without an upstream"
    );

    let remote_url = remote.to_str().expect("remote path is utf-8");
    git_success(temp.path(), &["clone", remote_url, other.to_str().unwrap()]);
    git_config_identity(&other);
    git_success(&other, &["checkout", "-B", "main", "origin/main"]);
    append_idea_sql(&other, "remote-idea", "Remote idea");
    git_success(&other, &["add", "-A"]);
    git_success(&other, &["commit", "-m", "Remote idea"]);
    git_success(&other, &["push", "origin", "main"]);

    insert_idea(&sidecar_root, "local-idea", "Local idea");
    git_success(&sidecar_root, &["fetch", "origin"]);

    let status_output = exo_direct_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "repo", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let status = json_result(&status_output);
    assert_eq!(status["issue_kind"].as_str(), Some("behind"), "{status:?}");
    assert!(status["next_actions"].as_array().is_some_and(|actions| {
        actions
            .iter()
            .any(|action| action["command"].as_str() == Some("exo sidecar repo sync"))
    }));

    let output = exo_direct_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "repo", "sync"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.repo.sync");
    assert_eq!(result["ok"], true, "{result:?}");
    assert_eq!(result["merged"], true, "{result:?}");
    assert_eq!(result["pushed"], true, "{result:?}");

    let upstream = git_output(&sidecar_root, &["rev-parse", "--abbrev-ref", "@{upstream}"]);
    assert_eq!(upstream.trim(), "origin/main");
    let remote_ideas = git_output(
        &remote,
        &[
            "show",
            "refs/heads/main:projects/external-test/agent-context/ideas.sql",
        ],
    );
    assert!(remote_ideas.contains("local-idea"), "{remote_ideas}");
    assert!(remote_ideas.contains("remote-idea"), "{remote_ideas}");
}

#[test]
fn sidecar_repo_sync_reports_same_row_conflict_without_push() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let remote = temp.path().join("sidecars.git");
    let other = temp.path().join("other-sidecar");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&remote).expect("create remote root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&remote);
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path is utf-8"),
        ],
    );
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Initial sidecar state",
        ])
        .assert()
        .success();
    git_success(&sidecar_root, &["push", "-u", "origin", "main"]);
    let original_remote_head = git_output(&remote, &["rev-parse", "refs/heads/main"]);

    let remote_url = remote.to_str().expect("remote path is utf-8");
    git_success(temp.path(), &["clone", remote_url, other.to_str().unwrap()]);
    git_config_identity(&other);
    git_success(&other, &["checkout", "-B", "main", "origin/main"]);
    append_idea_sql(&other, "same-idea", "Remote title");
    git_success(&other, &["add", "-A"]);
    git_success(&other, &["commit", "-m", "Remote idea"]);
    git_success(&other, &["push", "origin", "main"]);

    insert_idea(&sidecar_root, "same-idea", "Local title");

    let output = exo_direct_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "repo", "sync"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.repo.sync");
    assert_eq!(result["ok"], false, "{result:?}");
    assert_eq!(result["pushed"], false, "{result:?}");
    assert_eq!(result["conflicts"][0]["table"], "ideas_data");
    assert_eq!(result["conflicts"][0]["row_id"], "same-idea");
    let after_remote_head = git_output(&remote, &["rev-parse", "refs/heads/main"]);
    assert_ne!(after_remote_head, original_remote_head);
    assert_eq!(
        after_remote_head,
        git_output(&other, &["rev-parse", "HEAD"])
    );
}

#[test]
fn sidecar_repo_push_sets_upstream_for_explicit_branch_not_head() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let remote = temp.path().join("sidecars.git");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&remote).expect("create remote root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&remote);
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path is utf-8"),
        ],
    );
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    std::fs::write(sidecar_root.join("main.md"), "main\n").expect("write main file");
    git_success(&sidecar_root, &["add", "-A"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial sidecar"]);
    git_success(&sidecar_root, &["push", "-u", "origin", "main"]);
    git_success(&sidecar_root, &["checkout", "-b", "topic"]);
    std::fs::write(sidecar_root.join("topic.md"), "topic\n").expect("write topic file");
    git_success(&sidecar_root, &["add", "-A"]);
    git_success(&sidecar_root, &["commit", "-m", "Topic sidecar"]);
    git_success(&sidecar_root, &["checkout", "main"]);

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format", "json", "sidecar", "repo", "push", "--branch", "topic",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.repo.push");
    assert_eq!(result["branch"], "topic");
    assert_eq!(result["pushed"], true);
    let remote_ref = git_output(&remote, &["rev-parse", "refs/heads/topic"]);
    assert!(!remote_ref.trim().is_empty());
    let upstream = git_output(
        &sidecar_root,
        &["rev-parse", "--abbrev-ref", "topic@{upstream}"],
    );
    assert_eq!(upstream.trim(), "origin/topic");
}

#[test]
fn sidecar_repo_push_explicit_remote_ignores_first_remote_fallback() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let backup = temp.path().join("backup.git");
    let target = temp.path().join("target.git");
    let seeder = temp.path().join("backup-seeder");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&backup).expect("create backup remote root");
    std::fs::create_dir_all(&target).expect("create target remote root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&backup);
    git_init_bare(&target);

    git_success(
        temp.path(),
        &[
            "clone",
            backup.to_str().expect("backup path is utf-8"),
            seeder.to_str().expect("seeder path is utf-8"),
        ],
    );
    git_config_identity(&seeder);
    git_success(&seeder, &["checkout", "-B", "main"]);
    std::fs::write(seeder.join("backup.md"), "backup\n").expect("write backup file");
    git_success(&seeder, &["add", "-A"]);
    git_success(&seeder, &["commit", "-m", "Backup sidecar"]);
    git_success(&seeder, &["push", "origin", "main"]);

    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "backup",
            backup.to_str().expect("backup path is utf-8"),
        ],
    );
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "target",
            target.to_str().expect("target path is utf-8"),
        ],
    );
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Initial sidecar state",
        ])
        .assert()
        .success();
    git_success(&sidecar_root, &["fetch", "backup"]);
    assert_eq!(git_output(&sidecar_root, &["remote"]), "backup\ntarget\n");

    let upstream = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "@{upstream}"])
        .current_dir(&sidecar_root)
        .output()
        .expect("check upstream");
    assert!(
        !upstream.status.success(),
        "test setup should leave local main without an upstream"
    );

    let output = exo_direct_cmd(&repo, &home, &config_home)
        .args([
            "--format", "json", "sidecar", "repo", "push", "--remote", "target",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.repo.push");
    assert_eq!(result["remote"], "target");
    assert_eq!(result["branch"], "main");
    assert_eq!(result["pushed"], true);
    let upstream = git_output(&sidecar_root, &["rev-parse", "--abbrev-ref", "@{upstream}"]);
    assert_eq!(upstream.trim(), "target/main");
    let target_ref = git_output(&target, &["rev-parse", "refs/heads/main"]);
    assert!(!target_ref.trim().is_empty());
}

#[test]
fn sidecar_repo_push_refreshes_stale_tracking_when_remote_already_has_head() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let remote = temp.path().join("sidecars.git");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&remote).expect("create remote root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&remote);
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path is utf-8"),
        ],
    );
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    std::fs::write(sidecar_root.join("initial.md"), "initial\n").expect("write initial file");
    git_success(&sidecar_root, &["add", "-A"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial sidecar"]);
    git_success(&sidecar_root, &["push", "-u", "origin", "main"]);
    let initial_head = git_output(&sidecar_root, &["rev-parse", "HEAD"]);

    std::fs::write(sidecar_root.join("later.md"), "later\n").expect("write later file");
    git_success(&sidecar_root, &["add", "-A"]);
    git_success(&sidecar_root, &["commit", "-m", "Later sidecar"]);
    git_success(&sidecar_root, &["push", "origin", "main"]);
    let later_head = git_output(&sidecar_root, &["rev-parse", "HEAD"]);
    git_success(
        &sidecar_root,
        &[
            "update-ref",
            "refs/remotes/origin/main",
            initial_head.trim(),
        ],
    );

    let output = exo_direct_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "repo", "push"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.repo.push");
    assert_eq!(result["ok"], true, "{result:?}");
    assert_eq!(result["pushed"], false, "{result:?}");
    assert_eq!(result["already_synced"], true, "{result:?}");
    assert_eq!(
        git_output(&sidecar_root, &["rev-parse", "refs/remotes/origin/main"]),
        later_head
    );
}

#[test]
fn sidecar_repo_push_reports_remote_updates_without_raw_git_rejection() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let remote = temp.path().join("sidecars.git");
    let other = temp.path().join("other-sidecar");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&remote).expect("create remote root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&remote);
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path is utf-8"),
        ],
    );
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    std::fs::write(sidecar_root.join("initial.md"), "initial\n").expect("write initial file");
    git_success(&sidecar_root, &["add", "-A"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial sidecar"]);
    git_success(&sidecar_root, &["push", "-u", "origin", "main"]);

    let remote_url = remote.to_str().expect("remote path is utf-8");
    git_success(temp.path(), &["clone", remote_url, other.to_str().unwrap()]);
    git_config_identity(&other);
    git_success(&other, &["checkout", "-B", "main", "origin/main"]);
    std::fs::write(other.join("remote.md"), "remote\n").expect("write remote file");
    git_success(&other, &["add", "-A"]);
    git_success(&other, &["commit", "-m", "Remote sidecar update"]);
    git_success(&other, &["push", "origin", "main"]);

    std::fs::write(sidecar_root.join("local.md"), "local\n").expect("write local file");
    git_success(&sidecar_root, &["add", "-A"]);
    git_success(&sidecar_root, &["commit", "-m", "Local sidecar update"]);

    let output = exo_direct_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "repo", "push"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let envelope: JsonValue = serde_json::from_slice(&output).expect("valid json envelope");
    assert_eq!(envelope["status"], "error");
    let error = &envelope["error"];
    let message = error["message"].as_str().expect("error message");

    assert!(
        message.contains("sidecar repo has updates from origin/main"),
        "{message}"
    );
    assert!(message.contains("sidecar repo sync"), "{message}");
    assert!(!message.contains("[rejected]"), "{message}");
    assert!(!message.contains("cannot lock ref"), "{message}");
    assert!(envelope["steering"].as_array().is_some(), "{envelope:?}");
}

#[test]
fn sidecar_repo_push_without_remote_errors_clearly() {
    let temp = short_tempdir();
    let repo = temp.path().join("external repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecar root");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    git_init(&repo);
    git_init(&sidecar_root);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "repo", "push"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    assert!(
        error["message"]
            .as_str()
            .expect("message")
            .contains("requires an existing remote named 'origin'"),
        "{error:?}"
    );
}

#[test]
fn sidecar_repo_remote_adds_origin_remote() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let remote = temp.path().join("sidecars.git");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&remote).expect("create remote root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&remote);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    let remote_url = remote.to_str().expect("remote path is utf-8");
    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format", "json", "sidecar", "repo", "remote", "--url", remote_url,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.repo.remote");
    assert_eq!(result["remote"], "origin");
    assert_eq!(result["url"], remote_url);
    assert_eq!(result["previous_url"], JsonValue::Null);
    assert_eq!(result["changed"], true);
    assert_eq!(result["replaced"], false);
    assert_eq!(
        git_output(&sidecar_root, &["remote", "get-url", "origin"]).trim(),
        remote_url
    );
}

#[test]
fn sidecar_repo_remote_is_idempotent_for_same_url() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let remote = temp.path().join("sidecars.git");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&remote).expect("create remote root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&remote);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    let remote_url = remote.to_str().expect("remote path is utf-8");
    for _ in 0..2 {
        let output = exo_cmd(&repo, &home, &config_home)
            .args([
                "--format", "json", "sidecar", "repo", "remote", "--url", remote_url,
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let result = json_result(&output);

        assert_eq!(result["kind"], "sidecar.repo.remote");
        assert_eq!(result["remote"], "origin");
        assert_eq!(result["url"], remote_url);
    }

    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format", "json", "sidecar", "repo", "remote", "--url", remote_url,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["previous_url"], remote_url);
    assert_eq!(result["changed"], false);
    assert_eq!(result["replaced"], false);
}

#[test]
fn sidecar_repo_remote_rejects_conflicting_url_without_replace() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let first_remote = temp.path().join("first.git");
    let second_remote = temp.path().join("second.git");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&first_remote).expect("create first remote root");
    std::fs::create_dir_all(&second_remote).expect("create second remote root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&first_remote);
    git_init_bare(&second_remote);
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            first_remote.to_str().expect("remote path is utf-8"),
        ],
    );
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    let second_url = second_remote.to_str().expect("remote path is utf-8");
    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format", "json", "sidecar", "repo", "remote", "--url", second_url,
        ])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    assert!(
        error["message"]
            .as_str()
            .expect("message")
            .contains("already points to")
    );
    assert!(
        error["message"]
            .as_str()
            .expect("message")
            .contains("use --replace")
    );
    assert_eq!(
        git_output(&sidecar_root, &["remote", "get-url", "origin"]).trim(),
        first_remote.to_str().expect("remote path is utf-8")
    );
}

#[test]
fn sidecar_repo_remote_replaces_conflicting_url_with_replace() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("sidecars");
    let first_remote = temp.path().join("first.git");
    let second_remote = temp.path().join("second.git");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
    std::fs::create_dir_all(&first_remote).expect("create first remote root");
    std::fs::create_dir_all(&second_remote).expect("create second remote root");
    git_init(&repo);
    git_init(&sidecar_root);
    git_init_bare(&first_remote);
    git_init_bare(&second_remote);
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            first_remote.to_str().expect("remote path is utf-8"),
        ],
    );
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    let first_url = first_remote.to_str().expect("remote path is utf-8");
    let second_url = second_remote.to_str().expect("remote path is utf-8");
    let output = exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "remote",
            "--url",
            second_url,
            "--replace",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.repo.remote");
    assert_eq!(result["previous_url"], first_url);
    assert_eq!(result["url"], second_url);
    assert_eq!(result["changed"], true);
    assert_eq!(result["replaced"], true);
    assert_eq!(
        git_output(&sidecar_root, &["remote", "get-url", "origin"]).trim(),
        second_url
    );
}

#[test]
fn sidecar_bootstrap_commit_remote_pushes_to_local_bare_remote() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let remote = temp.path().join("sidecars.git");
    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&remote).expect("create remote root");
    git_init(&repo);
    git_init_bare(&remote);

    let bootstrap_output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "bootstrap"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let bootstrap_result = json_result(&bootstrap_output);
    let sidecar_root = Path::new(
        bootstrap_result["sidecar_root"]
            .as_str()
            .expect("sidecar root"),
    );
    git_config_identity(sidecar_root);

    exo_cmd(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "repo",
            "commit",
            "--message",
            "Bootstrap sidecar",
        ])
        .assert()
        .success();

    let remote_url = remote.to_str().expect("remote path is utf-8");
    exo_cmd(&repo, &home, &config_home)
        .args([
            "--format", "json", "sidecar", "repo", "remote", "--url", remote_url,
        ])
        .assert()
        .success();

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "repo", "push"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.repo.push");
    assert_eq!(result["remote"], "origin");
    let branch = result["branch"].as_str().expect("pushed branch");
    assert_eq!(result["pushed"], true);
    let remote_ref = git_output(&remote, &["rev-parse", &format!("refs/heads/{branch}")]);
    assert!(!remote_ref.trim().is_empty());
}

#[test]
fn sidecar_repo_requires_sidecar_policy() {
    let temp = short_tempdir();
    let repo = temp.path().join("external-repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "repo", "status"])
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let error = json_error(&output);

    assert!(
        error["message"]
            .as_str()
            .expect("message")
            .contains("require active sidecar policy")
    );
}

#[test]
fn sidecar_repo_parser_dispatches_status_commit_remote_push_and_sync() {
    use exo::command::command_spec::CommandSpec;
    use exo::command::registry::{build_command_from_invocation, default_registry};
    use exo::router::compile_argv;

    let spec = CommandSpec::from_registry(&default_registry());
    let repo_operation = spec
        .operation("sidecar", "repo")
        .expect("sidecar repo operation");
    assert_eq!(repo_operation.effect, exo::api::protocol::Effect::Write);

    let status_command = build_command_from_invocation(
        &compile_argv(
            &spec,
            &[
                "sidecar".to_string(),
                "repo".to_string(),
                "status".to_string(),
            ],
        )
        .invocation
        .expect("status invocation"),
        Path::new("."),
    )
    .expect("build status command")
    .expect("known status command");
    assert_eq!(status_command.effect(), exo::api::protocol::Effect::Pure);

    for args in [
        vec![
            "sidecar",
            "bootstrap",
            "--key",
            "locald",
            "--root",
            "/tmp/locald-sidecar",
        ],
        vec!["sidecar", "init", "--key", "locald"],
        vec![
            "sidecar",
            "link",
            "--key",
            "locald",
            "--root",
            "/tmp/locald-sidecar",
        ],
    ] {
        let argv = args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>();
        let compilation = compile_argv(&spec, &argv);
        assert!(
            compilation.diagnostics.is_empty(),
            "diagnostics for {args:?}: {:?}",
            compilation.diagnostics
        );
        let invocation = compilation.invocation.expect("invocation");
        let command = build_command_from_invocation(&invocation, Path::new("."))
            .expect("build sidecar command")
            .expect("known sidecar command");
        assert_eq!(command.effect(), exo::api::protocol::Effect::Write);
    }

    for args in [
        vec!["sidecar", "repo", "status"],
        vec!["sidecar", "repo", "commit", "--message", "Test"],
        vec![
            "sidecar",
            "repo",
            "remote",
            "--url",
            "https://example.invalid/sidecar.git",
            "--replace",
        ],
        vec!["sidecar", "repo", "push"],
        vec!["sidecar", "repo", "sync"],
    ] {
        let argv = args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>();
        let compilation = compile_argv(&spec, &argv);
        assert!(
            compilation.diagnostics.is_empty(),
            "diagnostics for {args:?}: {:?}",
            compilation.diagnostics
        );
        let invocation = compilation.invocation.expect("invocation");
        assert_eq!(invocation.namespace(), "sidecar");
        assert_eq!(invocation.operation(), "repo");
        assert_eq!(invocation.get_string("action"), Some(args[2]));
        let command = build_command_from_invocation(&invocation, Path::new("."))
            .expect("build command")
            .expect("known command");
        if args[2] == "status" {
            assert_eq!(command.effect(), exo::api::protocol::Effect::Pure);
        } else {
            assert_eq!(command.effect(), exo::api::protocol::Effect::Write);
        }
    }
}

#[test]
fn sidecar_setup_dry_run_plans_profile_registry_and_state_repo() {
    let temp = short_tempdir();
    let repo = temp.path().join("exo2");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("exo2-sidecar");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/wycats/exo2.git",
        ],
    );
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "setup", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["kind"], "sidecar.setup");
    assert_eq!(result["dry_run"], true);
    assert_eq!(result["profile_owner"], "wycats");
    assert_eq!(result["profile_repo"], "wycats/wycats");
    assert_eq!(result["profile_path"], ".exosuit/sidecars.toml");
    assert_eq!(result["state_repo"], "exo2-exosuit-state");
    assert_eq!(
        result["remote_url"],
        "git@github.com:wycats/exo2-exosuit-state.git"
    );
    assert!(
        result["registry_entry"]
            .as_str()
            .expect("registry entry")
            .contains("[projects.\"github.com/wycats/exo2\"]")
    );
    assert_eq!(result["created_repo"], false);
    assert_eq!(result["updated_registry"], false);
    assert_eq!(result["configured_remote"], false);
}

#[test]
fn sidecar_setup_dry_run_uses_rewritten_workspace_remote() {
    let temp = short_tempdir();
    let repo = temp.path().join("exo2");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("exo2-sidecar");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(&repo, &["remote", "add", "origin", "gh:wycats/exo2.git"]);
    git_success(&repo, &["config", "url.git@github.com:.insteadOf", "gh:"]);
    link_sidecar(&repo, &home, &config_home, &sidecar_root);

    let output = exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "setup", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let result = json_result(&output);

    assert_eq!(result["profile_owner"], "wycats");
    assert_eq!(result["state_repo"], "exo2-exosuit-state");
    assert!(
        result["registry_entry"]
            .as_str()
            .expect("registry entry")
            .contains("[projects.\"github.com/wycats/exo2\"]")
    );
}

#[test]
fn sidecar_setup_dry_run_is_compatible_with_matching_existing_remote() {
    let temp = short_tempdir();
    let repo = temp.path().join("exo2");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("exo2-sidecar");
    std::fs::create_dir_all(&repo).expect("create repo");
    git_init(&repo);
    git_success(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/wycats/exo2.git",
        ],
    );
    link_sidecar(&repo, &home, &config_home, &sidecar_root);
    git_init(&sidecar_root);
    git_success(
        &sidecar_root,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:wycats/exo2-exosuit-state.git",
        ],
    );

    exo_cmd(&repo, &home, &config_home)
        .args(["--format", "json", "sidecar", "setup", "--dry-run"])
        .assert()
        .success();
}
