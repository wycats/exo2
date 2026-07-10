#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

use exosuit_storage::OptionalExtension;
use serde_json::Value as JsonValue;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn git_success(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("run git command");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_config_identity(root: &Path) {
    git_success(root, &["config", "user.email", "test@example.com"]);
    git_success(root, &["config", "user.name", "Test User"]);
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

fn test_machine_identity() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

fn write_sidecar_write_owner_marker(
    sidecar_root: &Path,
    key: &str,
    pid: u32,
    workspace_root: &Path,
) {
    let state_root = sidecar_root.join("projects").join(key);
    let marker_path = sidecar_write_owner_marker_path(sidecar_root, key);
    fs::create_dir_all(marker_path.parent().expect("marker parent"))
        .expect("create owner marker dir");
    let marker = serde_json::json!({
        "version": 1,
        "sidecar_key": key,
        "sidecar_root": sidecar_root,
        "workspace_root": workspace_root,
        "state_root": state_root,
        "db_path": state_root.join("cache/exo.db"),
        "runtime_dir": state_root.join("runtime"),
        "pid": pid,
        "executable_path": null,
        "executable_blake3": null,
        "process_start_id": null,
        "machine": test_machine_identity(),
        "acquired_at_ms": 1,
        "refreshed_at_ms": 1,
    });
    fs::write(
        marker_path,
        serde_json::to_string_pretty(&marker).expect("serialize owner marker"),
    )
    .expect("write owner marker");
}

fn exo_cmd_with_home(root: &Path, home: &Path, config_home: &Path) -> assert_cmd::Command {
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("exo");
    cmd.current_dir(root)
        .arg("--direct")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", config_home);
    cmd
}

fn dogfood_exo_cmd(root: &Path) -> assert_cmd::Command {
    let home = root.join(".exo/test-home");
    let config_home = home.join("config");
    fs::create_dir_all(&config_home).expect("create dogfood test config home");
    exo_cmd_with_home(root, &home, &config_home)
}

fn json_result(output: Vec<u8>) -> JsonValue {
    let envelope: JsonValue = serde_json::from_slice(&output).expect("valid json envelope");
    assert_eq!(envelope["status"], "ok");
    envelope["result"].clone()
}

fn json_envelope(output: Vec<u8>) -> JsonValue {
    serde_json::from_slice(&output).expect("valid json envelope")
}

fn dogfood_verify(root: &Path) -> JsonValue {
    json_result(
        dogfood_exo_cmd(root)
            .args(["--format", "json", "dogfood", "verify"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
}

fn dogfood_receipt(root: &Path) -> JsonValue {
    json_result(
        dogfood_exo_cmd(root)
            .args(["--format", "json", "dogfood", "receipt"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
}

#[cfg(unix)]
fn dogfood_verify_with_workspace_proxy_path(root: &Path) -> JsonValue {
    json_result(
        exo_cmd_with_workspace_proxy_path(root)
            .args(["--format", "json", "dogfood", "verify"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
}

#[cfg(unix)]
fn dogfood_receipt_with_workspace_proxy_path(root: &Path) -> JsonValue {
    json_result(
        exo_cmd_with_workspace_proxy_path(root)
            .args(["--format", "json", "dogfood", "receipt"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
}

#[cfg(unix)]
fn exo_cmd_with_workspace_proxy_path(root: &Path) -> assert_cmd::Command {
    let mut cmd = dogfood_exo_cmd(root);
    cmd.env("PATH", path_with_workspace_proxy(root));
    cmd
}

#[cfg(unix)]
fn path_with_workspace_proxy(root: &Path) -> std::ffi::OsString {
    let mut paths = vec![root.join("target/debug")];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths).expect("join PATH")
}

fn path_with_git_only() -> std::ffi::OsString {
    #[cfg(windows)]
    {
        let output = Command::new("where")
            .arg("git")
            .output()
            .expect("find git on PATH");
        assert!(
            output.status.success(),
            "where git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let git_path = stdout
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .expect("where git should report a path");
        let git_dir = PathBuf::from(git_path)
            .parent()
            .expect("git path should have parent")
            .to_path_buf();
        std::env::join_paths([git_dir]).expect("join git PATH")
    }

    #[cfg(not(windows))]
    {
        std::env::join_paths([PathBuf::from("/usr/bin"), PathBuf::from("/bin")])
            .expect("join git PATH")
    }
}

fn write_plugin_mcp(root: &Path, command: &str, args: &[&str]) {
    let plugin_dir = root.join("plugins/exo");
    std::fs::create_dir_all(&plugin_dir).expect("create plugin dir");
    std::fs::write(
        plugin_dir.join(".mcp.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "mcpServers": {
                "exo": {
                    "command": command,
                    "args": args
                }
            }
        }))
        .expect("serialize plugin mcp config"),
    )
    .expect("write plugin mcp config");
}

#[cfg(unix)]
fn write_workspace_exo_mcp(root: &Path) -> PathBuf {
    let path = root.join("target/debug").join(exo_mcp_binary_name());
    fs::create_dir_all(path.parent().expect("proxy binary has parent"))
        .expect("create proxy binary parent");
    fs::write(&path, proxy_health_script("")).expect("write proxy binary");
    make_executable(&path);
    path
}

#[cfg(unix)]
fn proxy_health_script(extra: &str) -> String {
    let exo = assert_cmd::cargo::cargo_bin!("exo");
    let exo_hash = test_file_blake3(&exo);
    let exo_len = fs::metadata(&exo).expect("exo metadata").len();
    format!(
        "#!/bin/sh\nif [ \"$1\" = \"--proxy-health\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\n{extra}exit 0\n",
        serde_json::json!({
            "kind": "exo-mcp.proxy-health",
            "worker_protocol_version": exo::mcp::MCP_WORKER_PROTOCOL_VERSION,
            "status": {
                "worker": {
                    "identity": {
                        "executable_path": exo,
                        "executable_identity": {
                            "stable_hash": exo_hash,
                            "len": exo_len
                        }
                    }
                }
            }
        })
    )
}

#[cfg(unix)]
fn proxy_health_script_with_payload(payload: JsonValue, extra: &str) -> String {
    format!(
        "#!/bin/sh\nif [ \"$1\" = \"--proxy-health\" ]; then\n{extra}  printf '%s\\n' '{}'\n  exit 0\nfi\nexit 0\n",
        payload
    )
}

#[cfg(unix)]
fn healthy_proxy_payload() -> JsonValue {
    let exo = assert_cmd::cargo::cargo_bin!("exo");
    let exo_hash = test_file_blake3(&exo);
    let exo_len = fs::metadata(&exo).expect("exo metadata").len();
    serde_json::json!({
        "kind": "exo-mcp.proxy-health",
        "worker_protocol_version": exo::mcp::MCP_WORKER_PROTOCOL_VERSION,
        "status": {
            "worker": {
                "identity": {
                    "executable_path": exo,
                    "executable_identity": {
                        "stable_hash": exo_hash,
                        "len": exo_len
                    }
                }
            }
        }
    })
}

#[cfg(unix)]
fn test_file_blake3(path: &Path) -> String {
    blake3::hash(&fs::read(path).expect("read test binary"))
        .to_hex()
        .to_string()
}

#[cfg(unix)]
fn exo_mcp_binary_name() -> &'static str {
    if cfg!(windows) {
        "exo-mcp.exe"
    } else {
        "exo-mcp"
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(path).expect("proxy metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("set proxy executable bit");
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

fn dogfood_verify_with_home(root: &Path, home: &Path, config_home: &Path) -> JsonValue {
    let output = exo_cmd_with_home(root, home, config_home)
        .args(["--format", "json", "dogfood", "verify"])
        .output()
        .expect("run dogfood verify");
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected health-failure exit; stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
    json_result(output.stdout)
}

fn dogfood_health_failure_with_home(
    root: &Path,
    home: &Path,
    config_home: &Path,
    args: &[&str],
) -> JsonValue {
    let output = exo_cmd_with_home(root, home, config_home)
        .args(args)
        .output()
        .expect("run dogfood command");
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected health-failure exit; stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    json_result(output.stdout)
}

fn dogfood_success_with_home(
    root: &Path,
    home: &Path,
    config_home: &Path,
    args: &[&str],
) -> JsonValue {
    json_result(
        exo_cmd_with_home(root, home, config_home)
            .args(args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
}

#[test]
fn dogfood_verify_reports_canonical_project_runtime_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());

    let project = json_result(
        dogfood_exo_cmd(temp.path())
            .args(["--format", "json", "project", "resolve"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    let dogfood = dogfood_verify(temp.path());

    assert_eq!(dogfood["kind"], "dogfood.verify");
    assert_eq!(dogfood["ok"], true);
    assert_eq!(dogfood["project"]["id"], project["project"]["id"]);
    for field in ["state_root", "db_path", "runtime_dir", "socket_path"] {
        assert_eq!(dogfood["paths"][field], project["paths"][field]);
    }
}

#[test]
fn dogfood_verify_require_daemon_reports_stopped_when_daemon_is_absent() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());

    let regular = dogfood_verify(temp.path());
    assert_eq!(regular["ok"], true);
    assert_eq!(regular["daemon"]["required"], false);
    assert_eq!(regular["daemon"]["socket_connectable"], JsonValue::Null);

    let output = dogfood_exo_cmd(temp.path())
        .args(["--format", "json", "dogfood", "verify", "--require-daemon"])
        .output()
        .expect("run dogfood verify");
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected require-daemon health failure; stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let dogfood = json_result(output.stdout);
    assert_eq!(dogfood["kind"], "dogfood.verify");
    assert_eq!(dogfood["ok"], false);
    assert_eq!(dogfood["daemon"]["required"], true);
    assert_eq!(dogfood["daemon"]["ok"], false);
    assert_eq!(dogfood["daemon"]["state"], "stopped");
    assert_eq!(dogfood["daemon"]["socket_connectable"], false);
    assert_eq!(dogfood["daemon"]["identity_exists"], false);
    assert_eq!(dogfood["daemon"]["identity_readable"], false);
    assert_eq!(
        dogfood["daemon"]["identity_matches_workspace"],
        JsonValue::Null
    );
    assert_eq!(
        dogfood["daemon"]["identity_matches_executable"],
        JsonValue::Null
    );
    assert_eq!(dogfood["daemon"]["issue"], "daemon is stopped");
}

#[cfg(unix)]
#[test]
fn dogfood_receipt_writes_runtime_receipt_and_verify_compares_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);
    let proxy = write_workspace_exo_mcp(temp.path());

    let receipt = dogfood_receipt_with_workspace_proxy_path(temp.path());
    assert_eq!(receipt["ok"], true);
    let receipt_path = PathBuf::from(
        receipt["receipt_path"]
            .as_str()
            .expect("receipt path is string"),
    );
    assert!(receipt_path.exists(), "receipt should be written");

    let verify = dogfood_verify_with_workspace_proxy_path(temp.path());
    assert_eq!(verify["ok"], true);
    assert_eq!(verify["receipt"]["matches"], true);
    assert!(verify["plugin"]["blake3"].as_str().is_some());
    assert_eq!(verify["plugin"]["ok"], true);
    assert_eq!(verify["plugin"]["mcp_server"]["command"], "exo-mcp");
    assert_eq!(
        verify["plugin"]["mcp_server"]["args"],
        JsonValue::Array(vec![])
    );
    assert_eq!(verify["plugin"]["mcp_server"]["proxy_backed"], true);
    assert_eq!(
        verify["plugin"]["proxy_binary"]["path"]
            .as_str()
            .expect("proxy path is string"),
        proxy
            .canonicalize()
            .expect("canonical proxy path")
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(verify["plugin"]["proxy_binary"]["source"], "path");
    assert_eq!(verify["plugin"]["proxy_binary"]["executable"], true);
    assert!(
        verify["plugin"]["proxy_binary"]["blake3"]
            .as_str()
            .is_some()
    );
}

#[test]
fn dogfood_verify_requires_proxy_backed_plugin_when_plugin_package_exists() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo", &["mcp", "serve"]);

    let output = dogfood_exo_cmd(temp.path())
        .args(["--format", "json", "dogfood", "verify", "--skip-receipt"])
        .output()
        .expect("run dogfood verify");
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected plugin health failure; stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let dogfood = json_result(output.stdout);
    assert_eq!(dogfood["ok"], false);
    assert_eq!(dogfood["plugin"]["ok"], false);
    assert_eq!(dogfood["plugin"]["mcp_server"]["command"], "exo");
    assert_eq!(
        dogfood["plugin"]["mcp_server"]["args"],
        serde_json::json!(["mcp", "serve"])
    );
    assert_eq!(dogfood["plugin"]["mcp_server"]["proxy_backed"], false);
    assert_eq!(
        dogfood["plugin"]["issue"],
        "plugin MCP server must launch exo-mcp with no args"
    );
}

#[test]
fn dogfood_verify_requires_resolvable_exo_mcp_proxy_binary() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);

    let output = dogfood_exo_cmd(temp.path())
        .args(["--format", "json", "dogfood", "verify", "--skip-receipt"])
        .env("PATH", path_with_git_only())
        .output()
        .expect("run dogfood verify");
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected plugin health failure; stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let dogfood = json_result(output.stdout);
    assert_eq!(dogfood["ok"], false);
    assert_eq!(dogfood["plugin"]["ok"], false);
    assert_eq!(dogfood["plugin"]["mcp_server"]["proxy_backed"], true);
    assert_eq!(dogfood["plugin"]["proxy_binary"]["command"], "exo-mcp");
    assert_eq!(dogfood["plugin"]["proxy_binary"]["executable"], false);
    assert!(
        dogfood["plugin"]["issue"]
            .as_str()
            .expect("plugin issue")
            .contains("cargo install --path tools/exo --locked"),
        "{dogfood}"
    );
}

#[cfg(unix)]
#[test]
fn dogfood_verify_requires_built_proxy_to_be_on_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);
    write_workspace_exo_mcp(temp.path());

    let output = dogfood_exo_cmd(temp.path())
        .args(["--format", "json", "dogfood", "verify", "--skip-receipt"])
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("run dogfood verify");
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected plugin health failure; stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let dogfood = json_result(output.stdout);
    assert_eq!(dogfood["ok"], false);
    assert_eq!(dogfood["plugin"]["ok"], false);
    assert_eq!(dogfood["plugin"]["mcp_server"]["proxy_backed"], true);
    assert!(
        dogfood["plugin"]["issue"]
            .as_str()
            .expect("plugin issue")
            .contains("plugin host resolves `exo-mcp` through PATH"),
        "{dogfood}"
    );
}

#[cfg(unix)]
#[test]
fn dogfood_verify_accepts_installed_proxy_when_workspace_proxy_exists() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);
    write_workspace_exo_mcp(temp.path());

    let fake_bin = temp.path().join("fake-bin");
    fs::create_dir(&fake_bin).expect("create fake bin dir");
    let fake_proxy = fake_bin.join(exo_mcp_binary_name());
    fs::write(&fake_proxy, proxy_health_script("")).expect("write fake proxy");
    make_executable(&fake_proxy);

    let dogfood = json_result(
        dogfood_exo_cmd(temp.path())
            .args(["--format", "json", "dogfood", "verify", "--skip-receipt"])
            .env(
                "PATH",
                std::env::join_paths([
                    fake_bin,
                    temp.path().join("target/debug"),
                    PathBuf::from("/usr/bin"),
                    PathBuf::from("/bin"),
                ])
                .expect("join fake PATH"),
            )
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );

    assert_eq!(dogfood["ok"], true);
    assert_eq!(dogfood["plugin"]["ok"], true);
    assert_eq!(
        dogfood["plugin"]["proxy_binary"]["path"]
            .as_str()
            .expect("proxy path"),
        fake_proxy
            .canonicalize()
            .expect("canonical fake proxy")
            .to_string_lossy()
            .as_ref()
    );
}

#[cfg(unix)]
#[test]
fn dogfood_verify_accepts_absolute_proxy_command() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());

    let installed = temp.path().join("installed");
    fs::create_dir(&installed).expect("create installed dir");
    let proxy = installed.join(exo_mcp_binary_name());
    fs::write(&proxy, proxy_health_script("")).expect("write fake proxy");
    make_executable(&proxy);
    write_plugin_mcp(
        temp.path(),
        proxy.to_str().expect("proxy path is utf-8"),
        &[],
    );

    let dogfood = json_result(
        dogfood_exo_cmd(temp.path())
            .args(["--format", "json", "dogfood", "verify", "--skip-receipt"])
            .env("PATH", path_with_git_only())
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );

    assert_eq!(dogfood["ok"], true);
    assert_eq!(dogfood["plugin"]["ok"], true);
    assert_eq!(dogfood["plugin"]["mcp_server"]["proxy_backed"], true);
    assert_eq!(
        dogfood["plugin"]["proxy_binary"]["path"]
            .as_str()
            .expect("proxy path"),
        proxy
            .canonicalize()
            .expect("canonical proxy")
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(
        dogfood["plugin"]["proxy_binary"]["source"],
        "plugin-command"
    );
}

#[cfg(unix)]
#[test]
fn dogfood_verify_reports_non_executable_proxy_on_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);

    let fake_bin = temp.path().join("fake-bin");
    fs::create_dir(&fake_bin).expect("create fake bin dir");
    let fake_proxy = fake_bin.join(exo_mcp_binary_name());
    fs::write(&fake_proxy, proxy_health_script("")).expect("write fake proxy");

    let output = dogfood_exo_cmd(temp.path())
        .args(["--format", "json", "dogfood", "verify", "--skip-receipt"])
        .env(
            "PATH",
            std::env::join_paths([fake_bin, PathBuf::from("/usr/bin"), PathBuf::from("/bin")])
                .expect("join fake PATH"),
        )
        .output()
        .expect("run dogfood verify");
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected plugin health failure; stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let dogfood = json_result(output.stdout);
    assert_eq!(dogfood["ok"], false);
    assert_eq!(dogfood["plugin"]["ok"], false);
    assert_eq!(
        dogfood["plugin"]["proxy_binary"]["path"]
            .as_str()
            .expect("proxy path"),
        fake_proxy
            .canonicalize()
            .expect("canonical fake proxy")
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(dogfood["plugin"]["proxy_binary"]["executable"], false);
    assert!(
        dogfood["plugin"]["proxy_binary"]["size_bytes"]
            .as_u64()
            .is_some()
    );
    let issue = dogfood["plugin"]["issue"].as_str().expect("plugin issue");
    assert!(issue.contains("not executable"), "{dogfood}");
    assert!(!issue.contains("not found on PATH"), "{dogfood}");
}

#[cfg(unix)]
#[test]
fn dogfood_verify_skips_non_executable_path_placeholder_before_valid_proxy() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);

    let placeholder_bin = temp.path().join("placeholder-bin");
    let installed_bin = temp.path().join("installed-bin");
    fs::create_dir(&placeholder_bin).expect("create placeholder bin dir");
    fs::create_dir(&installed_bin).expect("create installed bin dir");
    fs::write(
        placeholder_bin.join(exo_mcp_binary_name()),
        proxy_health_script(""),
    )
    .expect("write placeholder proxy");
    let installed_proxy = installed_bin.join(exo_mcp_binary_name());
    fs::write(&installed_proxy, proxy_health_script("")).expect("write installed proxy");
    make_executable(&installed_proxy);

    let dogfood = json_result(
        dogfood_exo_cmd(temp.path())
            .args(["--format", "json", "dogfood", "verify", "--skip-receipt"])
            .env(
                "PATH",
                std::env::join_paths([
                    placeholder_bin,
                    installed_bin,
                    PathBuf::from("/usr/bin"),
                    PathBuf::from("/bin"),
                ])
                .expect("join fake PATH"),
            )
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );

    assert_eq!(dogfood["ok"], true);
    assert_eq!(
        dogfood["plugin"]["proxy_binary"]["path"]
            .as_str()
            .expect("proxy path"),
        installed_proxy
            .canonicalize()
            .expect("canonical installed proxy")
            .to_string_lossy()
            .as_ref()
    );
}

#[cfg(unix)]
#[test]
fn dogfood_verify_rejects_non_proxy_exo_mcp_on_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);
    let fake_bin = temp.path().join("fake-bin");
    fs::create_dir(&fake_bin).expect("create fake bin dir");
    let fake_proxy = fake_bin.join(exo_mcp_binary_name());
    fs::write(&fake_proxy, "#!/bin/sh\nexit 0\n").expect("write fake proxy");
    make_executable(&fake_proxy);

    let output = dogfood_exo_cmd(temp.path())
        .args(["--format", "json", "dogfood", "verify", "--skip-receipt"])
        .env(
            "PATH",
            std::env::join_paths([fake_bin, PathBuf::from("/usr/bin"), PathBuf::from("/bin")])
                .expect("join fake PATH"),
        )
        .output()
        .expect("run dogfood verify");
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected plugin health failure; stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let dogfood = json_result(output.stdout);
    assert_eq!(dogfood["ok"], false);
    assert_eq!(dogfood["plugin"]["ok"], false);
    assert!(
        dogfood["plugin"]["issue"]
            .as_str()
            .expect("plugin issue")
            .contains("proxy health probe did not return valid JSON"),
        "{dogfood}"
    );
}

#[cfg(unix)]
#[test]
fn dogfood_proxy_health_probe_removes_reexec_suppression() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);

    let fake_bin = temp.path().join("fake-bin");
    fs::create_dir(&fake_bin).expect("create fake bin dir");
    let fake_proxy = fake_bin.join(exo_mcp_binary_name());
    fs::write(
        &fake_proxy,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--proxy-health\" ]; then\n  if [ -n \"${{EXO_NO_REEXEC:-}}\" ]; then\n    printf 'not json\\n'\n    exit 0\n  fi\n  printf '%s\\n' '{}'\n  exit 0\nfi\nexit 0\n",
            healthy_proxy_payload()
        ),
    )
    .expect("write fake proxy");
    make_executable(&fake_proxy);

    let dogfood = json_result(
        dogfood_exo_cmd(temp.path())
            .args(["--format", "json", "dogfood", "verify", "--skip-receipt"])
            .env(
                "PATH",
                std::env::join_paths([fake_bin, PathBuf::from("/usr/bin"), PathBuf::from("/bin")])
                    .expect("join fake PATH"),
            )
            .env("EXO_NO_REEXEC", "1")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );

    assert_eq!(dogfood["ok"], true);
    assert_eq!(dogfood["plugin"]["ok"], true);
}

#[cfg(unix)]
#[test]
fn dogfood_proxy_health_probe_times_out() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);

    let fake_bin = temp.path().join("fake-bin");
    fs::create_dir(&fake_bin).expect("create fake bin dir");
    let fake_proxy = fake_bin.join(exo_mcp_binary_name());
    fs::write(
        &fake_proxy,
        "#!/bin/sh\nif [ \"$1\" = \"--proxy-health\" ]; then\n  sleep 60\nfi\nexit 0\n",
    )
    .expect("write hanging fake proxy");
    make_executable(&fake_proxy);

    let output = dogfood_exo_cmd(temp.path())
        .args(["--format", "json", "dogfood", "verify", "--skip-receipt"])
        .env(
            "PATH",
            std::env::join_paths([fake_bin, PathBuf::from("/usr/bin"), PathBuf::from("/bin")])
                .expect("join fake PATH"),
        )
        .output()
        .expect("run dogfood verify");
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected plugin health failure; stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let dogfood = json_result(output.stdout);
    assert_eq!(dogfood["ok"], false);
    let issue = dogfood["plugin"]["issue"].as_str().expect("plugin issue");
    assert!(issue.contains("proxy health probe timed out"), "{dogfood}");
}

#[cfg(unix)]
#[test]
fn dogfood_proxy_health_probe_discards_large_stderr() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);

    let fake_bin = temp.path().join("fake-bin");
    fs::create_dir(&fake_bin).expect("create fake bin dir");
    let fake_proxy = fake_bin.join(exo_mcp_binary_name());
    fs::write(
        &fake_proxy,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--proxy-health\" ]; then\n  dd if=/dev/zero bs=1024 count=256 1>&2 2>/dev/null\n  printf '%s\\n' '{}'\n  exit 0\nfi\nexit 0\n",
            healthy_proxy_payload()
        ),
    )
    .expect("write noisy fake proxy");
    make_executable(&fake_proxy);

    let dogfood = json_result(
        dogfood_exo_cmd(temp.path())
            .args(["--format", "json", "dogfood", "verify", "--skip-receipt"])
            .env(
                "PATH",
                std::env::join_paths([fake_bin, PathBuf::from("/usr/bin"), PathBuf::from("/bin")])
                    .expect("join fake PATH"),
            )
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );

    assert_eq!(dogfood["ok"], true);
    assert_eq!(dogfood["plugin"]["ok"], true);
}

#[cfg(unix)]
#[test]
fn dogfood_records_effective_reexeced_proxy_identity_from_health() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);

    let fake_bin = temp.path().join("fake-bin");
    fs::create_dir(&fake_bin).expect("create fake bin dir");
    let launcher_proxy = fake_bin.join(exo_mcp_binary_name());
    let effective_proxy = temp.path().join("target/debug").join(exo_mcp_binary_name());
    fs::create_dir_all(effective_proxy.parent().expect("effective proxy parent"))
        .expect("create effective proxy parent");
    fs::write(&effective_proxy, proxy_health_script("")).expect("write effective proxy");
    make_executable(&effective_proxy);
    let effective_hash = test_file_blake3(&effective_proxy);
    let effective_len = fs::metadata(&effective_proxy)
        .expect("effective proxy metadata")
        .len();

    let mut payload = healthy_proxy_payload();
    payload["status"]["proxy"] = serde_json::json!({
        "executable_path": effective_proxy,
        "executable_identity": {
            "stable_hash": effective_hash,
            "len": effective_len
        }
    });
    fs::write(
        &launcher_proxy,
        proxy_health_script_with_payload(payload, ""),
    )
    .expect("write launcher proxy");
    make_executable(&launcher_proxy);

    let dogfood = json_result(
        dogfood_exo_cmd(temp.path())
            .args(["--format", "json", "dogfood", "verify", "--skip-receipt"])
            .env(
                "PATH",
                std::env::join_paths([fake_bin, PathBuf::from("/usr/bin"), PathBuf::from("/bin")])
                    .expect("join fake PATH"),
            )
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );

    assert_eq!(dogfood["ok"], true);
    assert_eq!(dogfood["plugin"]["ok"], true);
    assert_eq!(
        dogfood["plugin"]["proxy_binary"]["path"]
            .as_str()
            .expect("proxy path"),
        effective_proxy
            .canonicalize()
            .expect("canonical effective proxy")
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(
        dogfood["plugin"]["proxy_binary"]["blake3"],
        test_file_blake3(&effective_proxy)
    );
    assert_eq!(dogfood["plugin"]["proxy_binary"]["source"], "proxy-health");
}

#[cfg(unix)]
#[test]
fn dogfood_rejects_proxy_health_with_stale_worker_identity() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);

    let fake_bin = temp.path().join("fake-bin");
    fs::create_dir(&fake_bin).expect("create fake bin dir");
    let fake_proxy = fake_bin.join(exo_mcp_binary_name());
    let mut payload = healthy_proxy_payload();
    payload["status"]["worker"]["identity"]["executable_identity"]["stable_hash"] =
        JsonValue::String("stale-worker-hash".to_string());
    fs::write(&fake_proxy, proxy_health_script_with_payload(payload, ""))
        .expect("write fake proxy");
    make_executable(&fake_proxy);

    let output = dogfood_exo_cmd(temp.path())
        .args(["--format", "json", "dogfood", "verify", "--skip-receipt"])
        .env(
            "PATH",
            std::env::join_paths([fake_bin, PathBuf::from("/usr/bin"), PathBuf::from("/bin")])
                .expect("join fake PATH"),
        )
        .output()
        .expect("run dogfood verify");
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected plugin health failure; stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let dogfood = json_result(output.stdout);
    assert_eq!(dogfood["ok"], false);
    assert_eq!(dogfood["plugin"]["ok"], false);
    assert!(
        dogfood["plugin"]["issue"]
            .as_str()
            .expect("plugin issue")
            .contains("different executable hash than the current exo binary"),
        "{dogfood}"
    );
}

#[cfg(unix)]
#[test]
fn dogfood_verify_reports_stale_dogfood_activation() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);

    let fake_bin = temp.path().join("fake-bin");
    fs::create_dir(&fake_bin).expect("create fake bin dir");
    let fake_proxy = fake_bin.join(exo_mcp_binary_name());
    let mut payload = healthy_proxy_payload();
    payload["status"]["activation"] = serde_json::json!({
        "configured": true,
        "ok": false,
        "state": "source_build_missing",
        "issue": "the source Exo build recorded by dogfood activation is unavailable; run `cargo dogfood-exo` from the source checkout"
    });
    fs::write(&fake_proxy, proxy_health_script_with_payload(payload, ""))
        .expect("write fake proxy");
    make_executable(&fake_proxy);

    let output = dogfood_exo_cmd(temp.path())
        .args(["--format", "json", "dogfood", "verify", "--skip-receipt"])
        .env(
            "PATH",
            std::env::join_paths([fake_bin, PathBuf::from("/usr/bin"), PathBuf::from("/bin")])
                .expect("join fake PATH"),
        )
        .output()
        .expect("run dogfood verify");
    assert_eq!(output.status.code(), Some(2));

    let dogfood = json_result(output.stdout);
    assert_eq!(dogfood["ok"], false);
    assert_eq!(dogfood["plugin"]["ok"], false);
    assert_eq!(
        dogfood["plugin"]["proxy_binary"]["activation"]["state"],
        "source_build_missing"
    );
    assert!(
        dogfood["plugin"]["issue"]
            .as_str()
            .expect("plugin issue")
            .contains("cargo dogfood-exo"),
        "{dogfood}"
    );
}

#[cfg(unix)]
#[test]
fn dogfood_proxy_health_probe_does_not_wait_for_background_stdout() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);

    let fake_bin = temp.path().join("fake-bin");
    fs::create_dir(&fake_bin).expect("create fake bin dir");
    let fake_proxy = fake_bin.join(exo_mcp_binary_name());
    fs::write(
        &fake_proxy,
        proxy_health_script_with_payload(healthy_proxy_payload(), "(sleep 60) &\n"),
    )
    .expect("write proxy with background stdout holder");
    make_executable(&fake_proxy);

    let dogfood = json_result(
        dogfood_exo_cmd(temp.path())
            .args(["--format", "json", "dogfood", "verify", "--skip-receipt"])
            .env(
                "PATH",
                std::env::join_paths([fake_bin, PathBuf::from("/usr/bin"), PathBuf::from("/bin")])
                    .expect("join fake PATH"),
            )
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );

    assert_eq!(dogfood["ok"], true);
    assert_eq!(dogfood["plugin"]["ok"], true);
}

#[test]
fn dogfood_receipt_refuses_unresolved_exo_mcp_proxy_binary() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);

    let output = dogfood_exo_cmd(temp.path())
        .args(["--format", "json", "dogfood", "receipt"])
        .env("PATH", path_with_git_only())
        .output()
        .expect("run dogfood receipt");
    assert!(
        !output.status.success(),
        "expected receipt failure; stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: JsonValue = serde_json::from_slice(&output.stdout).expect("valid json envelope");
    assert_eq!(envelope["status"], "error");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .expect("error message")
            .contains(
                "Refusing to save dogfood activation baseline while plugin health is failing"
            ),
        "{envelope}"
    );
}

#[cfg(unix)]
#[test]
fn dogfood_receipt_comparison_ignores_proxy_mtime_drift() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);
    write_workspace_exo_mcp(temp.path());

    let receipt = dogfood_receipt_with_workspace_proxy_path(temp.path());
    assert_eq!(receipt["ok"], true);
    let receipt_path = PathBuf::from(
        receipt["receipt_path"]
            .as_str()
            .expect("receipt path is string"),
    );
    let mut receipt_json: JsonValue =
        serde_json::from_str(&std::fs::read_to_string(&receipt_path).expect("read receipt"))
            .expect("parse receipt");
    receipt_json["plugin"]["proxy_binary"]["modified_unix_ms"] =
        JsonValue::Number(serde_json::Number::from(0));
    std::fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt_json).expect("serialize receipt"),
    )
    .expect("write receipt with mtime drift");

    let verify = dogfood_verify_with_workspace_proxy_path(temp.path());
    assert_eq!(verify["ok"], true);
    assert_eq!(verify["receipt"]["matches"], true);
}

#[cfg(unix)]
#[test]
fn dogfood_verify_detects_stale_proxy_binary_receipt() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());
    write_plugin_mcp(temp.path(), "exo-mcp", &[]);
    let proxy = write_workspace_exo_mcp(temp.path());

    let receipt = dogfood_receipt_with_workspace_proxy_path(temp.path());
    assert_eq!(receipt["ok"], true);
    fs::write(&proxy, proxy_health_script("echo changed\n")).expect("update proxy binary");
    make_executable(&proxy);

    let output = exo_cmd_with_workspace_proxy_path(temp.path())
        .args(["--format", "json", "dogfood", "verify"])
        .output()
        .expect("run dogfood verify");
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected stale proxy receipt health failure; stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let dogfood = json_result(output.stdout);
    assert_eq!(dogfood["ok"], false);
    assert_eq!(dogfood["receipt"]["matches"], false);
    assert!(
        dogfood["receipt"]["mismatches"]
            .as_array()
            .expect("receipt mismatches")
            .iter()
            .any(|mismatch| mismatch["field"] == "plugin.proxy_binary.blake3"),
        "{dogfood}"
    );
}

#[test]
fn dogfood_verify_skip_receipt_ignores_stale_receipt() {
    let temp = tempfile::tempdir().expect("tempdir");
    git_init(temp.path());
    test_support::exo_init(temp.path());

    let receipt = dogfood_receipt(temp.path());
    let receipt_path = PathBuf::from(
        receipt["receipt_path"]
            .as_str()
            .expect("receipt path is string"),
    );
    let mut receipt_json: JsonValue =
        serde_json::from_str(&std::fs::read_to_string(&receipt_path).expect("read receipt"))
            .expect("parse receipt");
    receipt_json["binary"]["blake3"] = JsonValue::String("definitely-stale".to_string());
    std::fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt_json).expect("serialize receipt"),
    )
    .expect("write stale receipt");

    dogfood_exo_cmd(temp.path())
        .args(["--format", "json", "dogfood", "verify"])
        .assert()
        .code(2);

    let verify = json_result(
        dogfood_exo_cmd(temp.path())
            .args(["--format", "json", "dogfood", "verify", "--skip-receipt"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    assert_eq!(verify["ok"], true);
    assert_eq!(verify["receipt_skipped"], true);
    assert!(verify["receipt"].is_null());
}

#[test]
fn dogfood_verify_detects_legacy_sidecar_db_with_extra_rows() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("portable-sidecars");

    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&home).expect("create home");
    std::fs::create_dir_all(&config_home).expect("create config home");
    git_init(&repo);
    test_support::exo_init(&repo);

    exo_cmd_with_home(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "link",
            "--key",
            "dogfood-test",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ])
        .assert()
        .success();

    let legacy_db_path: PathBuf = home
        .join(".exo")
        .join("sidecars")
        .join("dogfood-test")
        .join("cache/exo.db");
    std::fs::create_dir_all(legacy_db_path.parent().expect("legacy parent"))
        .expect("create legacy db parent");
    let legacy_db = exosuit_storage::open_database(&legacy_db_path).expect("open legacy db");
    legacy_db
        .connection()
        .execute(
            "INSERT INTO epochs (text_id, title, slug) VALUES ('legacy-epoch', 'Legacy Epoch', 'legacy-epoch')",
            [],
        )
        .expect("insert legacy row");

    let dogfood = dogfood_verify_with_home(&repo, &home, &config_home);

    assert_eq!(dogfood["ok"], false);
    assert_eq!(dogfood["split_brain"]["errors"], 1);
    assert_eq!(dogfood["split_brain"]["candidates"][0]["severity"], "error");
    assert_eq!(dogfood["repair"]["preview_command"], "exo dogfood repair");
}

#[test]
fn dogfood_repair_apply_replays_missing_goal_and_task_rows() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("portable-sidecars");

    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&home).expect("create home");
    std::fs::create_dir_all(&config_home).expect("create config home");
    git_init(&repo);
    test_support::exo_init(&repo);

    exo_cmd_with_home(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "link",
            "--key",
            "dogfood-test",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ])
        .assert()
        .success();
    assert!(
        Command::new("git")
            .args(["-C", sidecar_root.to_str().unwrap(), "init"])
            .status()
            .expect("git init sidecar")
            .success()
    );
    assert!(
        Command::new("git")
            .args([
                "-C",
                sidecar_root.to_str().unwrap(),
                "config",
                "user.email",
                "test@example.com"
            ])
            .status()
            .expect("git config user.email")
            .success()
    );
    assert!(
        Command::new("git")
            .args([
                "-C",
                sidecar_root.to_str().unwrap(),
                "config",
                "user.name",
                "Test User"
            ])
            .status()
            .expect("git config user.name")
            .success()
    );

    let canonical_db_path = sidecar_root
        .join("projects")
        .join("dogfood-test")
        .join("cache/exo.db");
    let legacy_db_path = home
        .join(".exo")
        .join("sidecars")
        .join("dogfood-test")
        .join("cache/exo.db");
    std::fs::create_dir_all(legacy_db_path.parent().expect("legacy parent"))
        .expect("create legacy db parent");
    let canonical_db = exosuit_storage::open_database(&canonical_db_path).expect("open canonical");
    let canonical_conn = canonical_db.connection();
    canonical_conn
        .execute(
            "INSERT OR IGNORE INTO epochs (text_id, title, slug)
             VALUES ('epoch-one', 'Epoch One', 'epoch-one')",
            [],
        )
        .expect("insert canonical epoch");
    let epoch_id: i64 = canonical_conn
        .query_row(
            "SELECT id FROM epochs_data WHERE text_id = 'epoch-one'",
            [],
            |row| row.get(0),
        )
        .expect("canonical epoch id");
    canonical_conn
        .execute(
            "INSERT OR IGNORE INTO phases (text_id, title, status, epoch_id, slug)
             VALUES ('phase-one', 'Phase One', 'in-progress', ?1, 'phase-one')",
            [epoch_id],
        )
        .expect("insert canonical phase");
    drop(canonical_db);
    std::fs::copy(&canonical_db_path, &legacy_db_path).expect("copy canonical to legacy");

    let legacy_db = exosuit_storage::open_database(&legacy_db_path).expect("open legacy db");
    let conn = legacy_db.connection();
    let phase_id: i64 = conn
        .query_row(
            "SELECT id FROM phases_data ORDER BY id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("phase id");
    conn.execute(
        "INSERT INTO goals (text_id, label, status, phase_id, kind, description)
         VALUES ('strike-legacy', 'Legacy Strike', 'in-progress', ?1, 'strike', 'Recovered from legacy split-brain')",
        [phase_id],
    )
    .expect("insert legacy goal");
    let goal_rowid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO tasks (text_id, title, status, goal_id, notes)
         VALUES ('strike-legacy::task-one', 'Recover missing task', 'pending', ?1, 'legacy note')",
        [goal_rowid],
    )
    .expect("insert legacy task");

    let preview = dogfood_health_failure_with_home(
        &repo,
        &home,
        &config_home,
        &["--format", "json", "dogfood", "repair"],
    );
    assert_eq!(preview["plan"]["totals"]["missing_goals"], 1);
    assert_eq!(preview["plan"]["totals"]["missing_tasks"], 1);

    let apply = dogfood_success_with_home(
        &repo,
        &home,
        &config_home,
        &["--format", "json", "dogfood", "repair", "--apply"],
    );
    assert_eq!(apply["ok"], true);
    assert_eq!(apply["applied"]["inserted_goals"], 1);
    assert_eq!(apply["applied"]["inserted_tasks"], 1);
    assert!(apply["applied"]["backup_path"].as_str().is_some());
    assert_eq!(
        apply["applied"]["sidecar_auto_persist"]["committed"], true,
        "{apply:?}"
    );
    assert_eq!(git_status_porcelain(&sidecar_root), "");

    let canonical_db = exosuit_storage::open_database(&canonical_db_path).expect("open canonical");
    let restored_goal: String = canonical_db
        .connection()
        .query_row(
            "SELECT label FROM goals_data WHERE text_id = 'strike-legacy'",
            [],
            |row| row.get(0),
        )
        .expect("restored goal");
    assert_eq!(restored_goal, "Legacy Strike");

    let verify = dogfood_success_with_home(
        &repo,
        &home,
        &config_home,
        &["--format", "json", "dogfood", "verify", "--skip-receipt"],
    );
    assert_eq!(verify["split_brain"]["errors"], 0);
}

#[test]
fn dogfood_repair_apply_checkpoint_failure_preserves_retry_steering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("portable-sidecars");

    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&home).expect("create home");
    std::fs::create_dir_all(&config_home).expect("create config home");
    git_init(&repo);
    test_support::exo_init(&repo);

    exo_cmd_with_home(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "link",
            "--key",
            "dogfood-test",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ])
        .assert()
        .success();
    assert!(
        Command::new("git")
            .args(["-C", sidecar_root.to_str().unwrap(), "init"])
            .status()
            .expect("git init sidecar")
            .success()
    );

    let canonical_db_path = sidecar_root
        .join("projects")
        .join("dogfood-test")
        .join("cache/exo.db");
    let legacy_db_path = home
        .join(".exo")
        .join("sidecars")
        .join("dogfood-test")
        .join("cache/exo.db");
    std::fs::create_dir_all(legacy_db_path.parent().expect("legacy parent"))
        .expect("create legacy db parent");
    let canonical_db = exosuit_storage::open_database(&canonical_db_path).expect("open canonical");
    let canonical_conn = canonical_db.connection();
    canonical_conn
        .execute(
            "INSERT OR IGNORE INTO epochs (text_id, title, slug)
             VALUES ('epoch-one', 'Epoch One', 'epoch-one')",
            [],
        )
        .expect("insert canonical epoch");
    let epoch_id: i64 = canonical_conn
        .query_row(
            "SELECT id FROM epochs_data WHERE text_id = 'epoch-one'",
            [],
            |row| row.get(0),
        )
        .expect("canonical epoch id");
    canonical_conn
        .execute(
            "INSERT OR IGNORE INTO phases (text_id, title, status, epoch_id, slug)
             VALUES ('phase-one', 'Phase One', 'in-progress', ?1, 'phase-one')",
            [epoch_id],
        )
        .expect("insert canonical phase");
    drop(canonical_db);
    std::fs::copy(&canonical_db_path, &legacy_db_path).expect("copy canonical to legacy");

    let legacy_db = exosuit_storage::open_database(&legacy_db_path).expect("open legacy db");
    let conn = legacy_db.connection();
    let phase_id: i64 = conn
        .query_row(
            "SELECT id FROM phases_data ORDER BY id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("phase id");
    conn.execute(
        "INSERT INTO goals (text_id, label, status, phase_id, kind, description)
         VALUES ('strike-legacy', 'Legacy Strike', 'in-progress', ?1, 'strike', 'Recovered from legacy split-brain')",
        [phase_id],
    )
    .expect("insert legacy goal");
    drop(legacy_db);

    let projection_dir = sidecar_root
        .join("projects")
        .join("dogfood-test")
        .join("agent-context");
    if projection_dir.is_dir() {
        std::fs::remove_dir_all(&projection_dir).expect("remove projection dir");
    }
    std::fs::write(&projection_dir, "not a directory\n").expect("replace projection dir with file");

    let output = exo_cmd_with_home(&repo, &home, &config_home)
        .args(["--format", "json", "dogfood", "repair", "--apply"])
        .output()
        .expect("run dogfood repair apply");
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected checkpoint failure; stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = json_envelope(output.stdout);
    assert_eq!(envelope["status"], "error", "{envelope:?}");
    assert_eq!(
        envelope["error"]["code"], "precondition_failed",
        "{envelope:?}"
    );
    assert_eq!(
        envelope["error"]["details"]["details"]["kind"], "sidecar.local_checkpoint",
        "{envelope:?}"
    );
    assert!(
        envelope["error"]["details"]["steering"]["next_actions"]
            .as_array()
            .expect("steering next actions")
            .iter()
            .any(|action| action["command"] == "exo sidecar checkpoint"),
        "{envelope:?}"
    );

    let canonical_db = exosuit_storage::open_database(&canonical_db_path).expect("open canonical");
    let restored_goal: String = canonical_db
        .connection()
        .query_row(
            "SELECT label FROM goals_data WHERE text_id = 'strike-legacy'",
            [],
            |row| row.get(0),
        )
        .expect("restored goal");
    assert_eq!(restored_goal, "Legacy Strike");
}

#[test]
fn dogfood_repair_apply_preflights_sidecar_ownership_before_commit() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let other_workspace = temp.path().join("other-workspace");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("portable-sidecars");

    fs::create_dir_all(&repo).expect("create repo");
    fs::create_dir_all(&other_workspace).expect("create other workspace");
    fs::create_dir_all(&home).expect("create home");
    fs::create_dir_all(&config_home).expect("create config home");
    git_init(&repo);
    test_support::exo_init(&repo);

    exo_cmd_with_home(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "link",
            "--key",
            "dogfood-test",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ])
        .assert()
        .success();
    git_init(&sidecar_root);

    let canonical_db_path = sidecar_root
        .join("projects")
        .join("dogfood-test")
        .join("cache/exo.db");
    let legacy_db_path = home
        .join(".exo")
        .join("sidecars")
        .join("dogfood-test")
        .join("cache/exo.db");
    fs::create_dir_all(legacy_db_path.parent().expect("legacy parent"))
        .expect("create legacy db parent");
    let canonical_db = exosuit_storage::open_database(&canonical_db_path).expect("open canonical");
    let canonical_conn = canonical_db.connection();
    canonical_conn
        .execute(
            "INSERT OR IGNORE INTO epochs (text_id, title, slug)
             VALUES ('epoch-one', 'Epoch One', 'epoch-one')",
            [],
        )
        .expect("insert canonical epoch");
    let epoch_id: i64 = canonical_conn
        .query_row(
            "SELECT id FROM epochs_data WHERE text_id = 'epoch-one'",
            [],
            |row| row.get(0),
        )
        .expect("canonical epoch id");
    canonical_conn
        .execute(
            "INSERT OR IGNORE INTO phases (text_id, title, status, epoch_id, slug)
             VALUES ('phase-one', 'Phase One', 'in-progress', ?1, 'phase-one')",
            [epoch_id],
        )
        .expect("insert canonical phase");
    drop(canonical_db);
    fs::copy(&canonical_db_path, &legacy_db_path).expect("copy canonical to legacy");

    let legacy_db = exosuit_storage::open_database(&legacy_db_path).expect("open legacy db");
    let conn = legacy_db.connection();
    let phase_id: i64 = conn
        .query_row(
            "SELECT id FROM phases_data ORDER BY id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("phase id");
    conn.execute(
        "INSERT INTO goals (text_id, label, status, phase_id, kind, description)
         VALUES ('strike-legacy', 'Legacy Strike', 'in-progress', ?1, 'strike', 'Recovered from legacy split-brain')",
        [phase_id],
    )
    .expect("insert legacy goal");
    drop(legacy_db);

    write_sidecar_write_owner_marker(
        &sidecar_root,
        "dogfood-test",
        std::process::id(),
        &other_workspace,
    );

    let output = exo_cmd_with_home(&repo, &home, &config_home)
        .args(["--format", "json", "dogfood", "repair", "--apply"])
        .output()
        .expect("run dogfood repair apply");
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected preflight ownership failure; stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = json_envelope(output.stdout);
    assert_eq!(envelope["status"], "error", "{envelope:?}");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("another active runtime")),
        "{envelope:?}"
    );

    let canonical_db = exosuit_storage::open_database(&canonical_db_path).expect("open canonical");
    let restored_goal_exists: bool = canonical_db
        .connection()
        .query_row(
            "SELECT 1 FROM goals_data WHERE text_id = 'strike-legacy'",
            [],
            |_| Ok(true),
        )
        .optional()
        .expect("query restored goal")
        .unwrap_or(false);
    assert!(
        !restored_goal_exists,
        "dogfood repair must not commit before sidecar ownership is available"
    );
}

#[test]
fn dogfood_verify_fails_when_sidecar_repo_is_dirty() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("portable-sidecars");

    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&home).expect("create home");
    std::fs::create_dir_all(&config_home).expect("create config home");
    git_init(&repo);
    test_support::exo_init(&repo);

    exo_cmd_with_home(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "link",
            "--key",
            "dogfood-test",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ])
        .assert()
        .success();

    std::fs::write(sidecar_root.join("dirty.txt"), "dirty\n").expect("dirty sidecar");
    let verify = dogfood_health_failure_with_home(
        &repo,
        &home,
        &config_home,
        &["--format", "json", "dogfood", "verify", "--skip-receipt"],
    );
    assert_eq!(verify["ok"], false);
    assert_eq!(verify["portability"]["errors"], 1);
    assert_eq!(verify["portability"]["sidecar_git"]["severity"], "error");
}

#[test]
fn dogfood_verify_fails_when_sidecar_repo_has_foreign_checkpoint_debt() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let home = temp.path().join("home");
    let config_home = temp.path().join("config");
    let sidecar_root = temp.path().join("portable-sidecars");

    std::fs::create_dir_all(&repo).expect("create repo");
    std::fs::create_dir_all(&home).expect("create home");
    std::fs::create_dir_all(&config_home).expect("create config home");
    git_init(&repo);
    test_support::exo_init(&repo);

    exo_cmd_with_home(&repo, &home, &config_home)
        .args([
            "--format",
            "json",
            "sidecar",
            "link",
            "--key",
            "dogfood-test",
            "--root",
            sidecar_root.to_str().expect("sidecar root is utf-8"),
        ])
        .assert()
        .success();
    git_init(&sidecar_root);
    git_config_identity(&sidecar_root);
    git_success(&sidecar_root, &["add", "-A"]);
    git_success(&sidecar_root, &["commit", "-m", "Initial dogfood sidecar"]);

    let foreign_debt = sidecar_root.join("projects/sandboxd/agent-context/tasks.sql");
    std::fs::create_dir_all(foreign_debt.parent().expect("foreign debt parent"))
        .expect("create foreign projection dir");
    std::fs::write(&foreign_debt, "sandboxd checkpoint debt\n").expect("write foreign debt");

    let verify = dogfood_health_failure_with_home(
        &repo,
        &home,
        &config_home,
        &["--format", "json", "dogfood", "verify", "--skip-receipt"],
    );
    assert_eq!(verify["ok"], false);
    assert_eq!(verify["portability"]["errors"], 1);
    assert_eq!(verify["portability"]["sidecar_git"]["severity"], "error");
    assert_eq!(
        verify["portability"]["sidecar_git"]["status"]["issue_kind"].as_str(),
        Some("foreign_checkpoint_debt"),
        "{verify:?}"
    );
}
