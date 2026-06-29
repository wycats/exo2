//! Command tests for the context namespace.

use exo::command::context::ContextPaths;
use exo::command::{Command, CommandContext, OutputFormat};
use exo::project::{Project, ProjectId, SidecarAutoPushPolicy, StatePolicy};

#[test]
fn context_paths_command_returns_expected_paths() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let ctx = CommandContext {
        root: temp_dir.path(),
        project: None,
        format: OutputFormat::Json,
        agent_id: None,
        workflow_confirmation: None,
    };

    let output = ContextPaths::new()
        .execute(&ctx)
        .expect("context paths command should succeed");

    let data = output.data;
    assert_eq!(
        data["kind"],
        serde_json::Value::String("context.paths".to_string())
    );
    assert_eq!(data["ok"], serde_json::Value::Bool(true));

    let paths = data["paths"]
        .as_object()
        .expect("paths should be an object");

    assert_eq!(
        data["policy"],
        serde_json::Value::String("repo".to_string())
    );

    let expected = [
        ("plan", "docs/agent-context/epochs.sql"),
        ("tasks", "docs/agent-context/tasks.sql"),
        ("ideas", "docs/agent-context/ideas.sql"),
        ("axioms", "docs/agent-context/axioms.sql"),
    ];

    assert_eq!(paths.len(), expected.len());

    for (key, value) in expected {
        let entry = paths
            .get(key)
            .and_then(|path| path.as_str())
            .unwrap_or("<missing>");
        assert_eq!(entry, value, "path mismatch for {key}");
    }
}

#[test]
fn context_paths_command_uses_sidecar_projection_for_sidecar_policy() {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let sidecar_dir = tempfile::tempdir().expect("sidecar tempdir should be created");
    let sidecar_root = sidecar_dir.path().join("sidecars");
    let project = Project {
        id: ProjectId::from_git_common_dir(&temp_dir.path().join(".git")),
        git_common_dir: temp_dir.path().join(".git"),
        workspace_root: Some(temp_dir.path().to_path_buf()),
        policy: StatePolicy::Sidecar,
        projects_config_path: None,
        state_root: temp_dir.path().join("state"),
        sidecar_key: Some("demo".to_string()),
        sidecar_root: Some(sidecar_root.clone()),
        sidecar_auto_commit: true,
        sidecar_auto_push: SidecarAutoPushPolicy::IfRemote,
    };
    let ctx = CommandContext {
        root: temp_dir.path(),
        project: Some(&project),
        format: OutputFormat::Json,
        agent_id: None,
        workflow_confirmation: None,
    };

    let output = ContextPaths::new()
        .execute(&ctx)
        .expect("context paths command should succeed");

    let data = output.data;
    assert_eq!(
        data["policy"],
        serde_json::Value::String("sidecar".to_string())
    );
    assert_eq!(
        data["projection"]["kind"],
        serde_json::Value::String("sidecar_sql_projection".to_string())
    );
    let expected_tasks = sidecar_root
        .join("projects/demo/agent-context/tasks.sql")
        .to_string_lossy()
        .replace('\\', "/");
    assert_eq!(
        data["paths"]["tasks"].as_str(),
        Some(expected_tasks.as_str())
    );
}
