//! Machine-channel docs.links operations.

#![allow(clippy::assertions_on_constants)]
#![allow(clippy::disallowed_methods)]

#[macro_use]
mod test_support;

use exo::api::handler::handle_request_with_project;
use exo::api::protocol::RequestEnvelope;
use exo::project::{ProjectId, ProjectResolver};
use serde_json::Value as JsonValue;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use test_support::run_machine_channel_in_process;

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at tools/exo; repo root is two levels up.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let _ = p.pop();
    let _ = p.pop();
    p
}

fn run_channel(request_json: &str) -> JsonValue {
    let request: RequestEnvelope = ok_or_return!(
        serde_json::from_str(request_json),
        "expected valid request envelope";
        JsonValue::Null
    );

    let resp = run_machine_channel_in_process(&repo_root(), &request);
    ok_or_return!(
        serde_json::to_value(resp),
        "expected response to serialize";
        JsonValue::Null
    )
}

fn run_channel_with_project(
    workspace_root: &Path,
    project: &exo::project::Project,
    request_json: &str,
) -> JsonValue {
    let request: RequestEnvelope = ok_or_return!(
        serde_json::from_str(request_json),
        "expected valid request envelope";
        JsonValue::Null
    );

    let resp = handle_request_with_project(workspace_root, Some(project), request);
    ok_or_return!(
        serde_json::to_value(resp),
        "expected response to serialize";
        JsonValue::Null
    )
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

fn rel_to_repo_root(abs: &Path) -> String {
    let root = repo_root();
    let rel = ok_or_return!(
        abs.strip_prefix(&root),
        "path under repo root";
        abs.to_string_lossy().replace('\\', "/")
    );

    rel.to_string_lossy().replace('\\', "/")
}

fn write_md(path: &Path, contents: &str) {
    let parent = some_or_return!(path.parent(), "expected parent");
    assert!(fs::create_dir_all(parent).is_ok());
    assert!(fs::write(path, contents).is_ok());
}

#[test]
fn docs_links_check_returns_steering_to_fix_when_changes_exist() {
    let tmp = ok_or_return!(
        tempfile::Builder::new()
            .prefix("exo-docs-links-")
            .tempdir_in(repo_root()),
        "failed to create tempdir"
    );

    let md_path = tmp.path().join("input.md");
    write_md(&md_path, "See [RFC](exo:rfc/8) for details.\n");

    let targets_path = rel_to_repo_root(tmp.path());

    let resp = run_channel(&format!(
        "{{\"protocol_version\":1,\"id\":\"t1\",\"op\":{{\"kind\":\"call\",\"params\":{{\"address\":{{\"kind\":\"operation\",\"path\":[\"docs\",\"links\",\"check\"]}},\"input\":{{\"targets\":{{\"paths\":[\"{targets_path}\"]}},\"options\":{{\"strict\":true}}}}}}}}}}"
    ));

    assert_eq!(resp.get("status").and_then(|v| v.as_str()), Some("ok"));

    let result = some_or_return!(resp.get("result"), "expected result");
    assert_eq!(result.get("ok").and_then(JsonValue::as_bool), Some(false));
    assert!(
        result
            .get("changes")
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty()),
        "expected at least one planned change"
    );

    let steering = some_or_return!(resp.get("steering"), "expected steering");
    let next_call = some_or_return!(steering.get("next_call"), "expected next_call");
    assert_eq!(next_call.get("kind").and_then(|v| v.as_str()), Some("call"));

    let params = some_or_return!(next_call.get("params"), "expected params");
    let address = some_or_return!(params.get("address"), "expected address");
    let path = some_or_return!(
        address.get("path").and_then(|v| v.as_array()),
        "expected path"
    );
    let segs: Vec<&str> = path.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(segs, vec!["docs", "links", "fix"]);
}

#[test]
fn docs_links_fix_rewrites_exo_links_in_target_files() {
    let tmp = ok_or_return!(
        tempfile::Builder::new()
            .prefix("exo-docs-links-")
            .tempdir_in(repo_root()),
        "failed to create tempdir"
    );

    let md_path = tmp.path().join("input.md");
    write_md(&md_path, "See [RFC](exo:rfc/8) for details.\n");

    let targets_path = rel_to_repo_root(tmp.path());

    let resp = run_channel(&format!(
        "{{\"protocol_version\":1,\"id\":\"t2\",\"op\":{{\"kind\":\"call\",\"params\":{{\"address\":{{\"kind\":\"operation\",\"path\":[\"docs\",\"links\",\"fix\"]}},\"input\":{{\"targets\":{{\"paths\":[\"{targets_path}\"]}},\"options\":{{\"strict\":true}}}}}}}}}}"
    ));

    assert_eq!(resp.get("status").and_then(|v| v.as_str()), Some("ok"));

    let result = some_or_return!(resp.get("result"), "expected result");
    assert_eq!(result.get("ok").and_then(JsonValue::as_bool), Some(true));

    let updated = ok_or_return!(fs::read_to_string(&md_path), "failed to read updated md");
    assert!(updated.contains(".md)"));
    assert!(!updated.contains("exo:rfc/8"));
}

#[test]
fn docs_links_check_ignores_placeholder_exo_ellipsis() {
    let tmp = ok_or_return!(
        tempfile::Builder::new()
            .prefix("exo-docs-links-")
            .tempdir_in(repo_root()),
        "failed to create tempdir"
    );

    let md_path = tmp.path().join("input.md");
    write_md(&md_path, "Example: [Link](exo:...)\n");

    let targets_path = rel_to_repo_root(tmp.path());

    let resp = run_channel(&format!(
        "{{\"protocol_version\":1,\"id\":\"t3\",\"op\":{{\"kind\":\"call\",\"params\":{{\"address\":{{\"kind\":\"operation\",\"path\":[\"docs\",\"links\",\"check\"]}},\"input\":{{\"targets\":{{\"paths\":[\"{targets_path}\"]}},\"options\":{{\"strict\":true}}}}}}}}}}"
    ));

    assert_eq!(resp.get("status").and_then(|v| v.as_str()), Some("ok"));
    let result = some_or_return!(resp.get("result"), "expected result");
    assert_eq!(result.get("ok").and_then(JsonValue::as_bool), Some(true));
    assert!(
        result
            .get("diagnostics")
            .and_then(|v| v.as_array())
            .is_some_and(Vec::is_empty),
        "expected no diagnostics"
    );
    assert!(
        result
            .get("changes")
            .and_then(|v| v.as_array())
            .is_some_and(Vec::is_empty),
        "expected no changes"
    );
}

#[test]
fn docs_links_context_links_do_not_rewrite_to_external_sidecar_projection() {
    let tmp = ok_or_return!(
        tempfile::Builder::new().prefix("exo-docs-links-").tempdir(),
        "failed to create tempdir"
    );
    let repo = tmp.path().join("repo");
    let sidecar_root = tmp.path().join("sidecar root");
    let home = tmp.path().join("home");
    let config_home = tmp.path().join("config");
    assert!(fs::create_dir_all(&repo).is_ok());
    assert!(fs::create_dir_all(&sidecar_root).is_ok());
    assert!(fs::create_dir_all(&home).is_ok());
    assert!(fs::create_dir_all(config_home.join("exo")).is_ok());
    git_init(&repo);

    let id = ProjectId::from_git_common_dir(&git_common_dir(&repo));
    let policy = format!(
        "[projects.{}]\nstate = \"sidecar\"\nsidecar_key = \"docs-links-test\"\nsidecar_root = {:?}\n",
        id.as_str(),
        sidecar_root.to_string_lossy()
    );
    assert!(fs::write(config_home.join("exo/projects.toml"), policy).is_ok());
    let project = ProjectResolver::default()
        .with_home_dir(&home)
        .with_config_home(&config_home)
        .resolve(&repo)
        .expect("resolve sidecar project");

    let projection_dir = sidecar_root.join("projects/docs-links-test/agent-context");
    assert!(fs::create_dir_all(&projection_dir).is_ok());
    assert!(fs::write(projection_dir.join("tasks.sql"), "-- test\n").is_ok());

    let md_path = repo.join("input.md");
    write_md(&md_path, "See [tasks](exo:context/tasks).\n");

    let targets_path = "input.md";

    let resp = run_channel_with_project(
        &repo,
        &project,
        &format!(
            "{{\"protocol_version\":1,\"id\":\"t4\",\"op\":{{\"kind\":\"call\",\"params\":{{\"address\":{{\"kind\":\"operation\",\"path\":[\"docs\",\"links\",\"check\"]}},\"input\":{{\"targets\":{{\"paths\":[\"{targets_path}\"]}},\"options\":{{\"strict\":false}}}}}}}}}}"
        ),
    );

    assert_eq!(resp.get("status").and_then(|v| v.as_str()), Some("ok"));
    let result = some_or_return!(resp.get("result"), "expected result");
    eprintln!("result={}", result);
    assert_eq!(result.get("ok").and_then(JsonValue::as_bool), Some(false));
    assert!(
        result
            .get("diagnostics")
            .and_then(|v| v.as_array())
            .is_some_and(|diagnostics| diagnostics.iter().any(|diagnostic| {
                diagnostic.get("code").and_then(|v| v.as_str())
                    == Some("context_external_projection")
            })),
        "expected external projection diagnostic"
    );
    assert!(
        result
            .get("changes")
            .and_then(|v| v.as_array())
            .is_some_and(Vec::is_empty),
        "expected no rewrite for external sidecar projection"
    );
}
