use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use crate::api::protocol::Effect;
use crate::command::sidecar::{self, SidecarAutoPersistReport};
use crate::command_reference::ExoCommandReference;
use crate::failure::ExoFailure;
use crate::project::{Project, StatePolicy};
use crate::steering::{SuggestedAction, WorkIntent};
use anyhow::Result as ExoResult;
use fs2::FileExt;
use serde::Serialize;

pub fn should_write_sql_dump(namespace: &str, operation: &str, effect: Effect) -> bool {
    if effect == Effect::Pure {
        return false;
    }

    if namespace == "project" && operation == "resolve" {
        return false;
    }

    if namespace == "project" && operation == "repair-apply" {
        return false;
    }

    if namespace == "project" && operation == "move-root" {
        return false;
    }

    namespace != "sidecar" && namespace != "dogfood" && namespace != "storage"
}

pub fn write_sql_dump_after_success(
    workspace_root: &Path,
    project: Option<&Project>,
    namespace: &str,
    operation: &str,
    effect: Effect,
) -> ExoResult<bool> {
    if !should_write_sql_dump(namespace, operation, effect) {
        return Ok(false);
    }

    crate::context::write_sql_dump_with_project(workspace_root, project);
    Ok(true)
}

pub fn should_auto_persist_after_success(
    effect: Effect,
    namespace: &str,
    operation: &str,
    project: Option<&Project>,
) -> bool {
    should_write_sql_dump(namespace, operation, effect)
        && project.is_some_and(|project| project.policy == StatePolicy::Sidecar)
}

pub fn should_auto_persist_after_command_event(
    event_logged: bool,
    effect: Effect,
    namespace: &str,
    operation: &str,
    project: Option<&Project>,
) -> bool {
    event_logged && should_auto_persist_after_success(effect, namespace, operation, project)
}

pub fn should_log_command_event(namespace: &str, _operation: &str) -> bool {
    namespace != "sidecar" && namespace != "dogfood" && namespace != "storage"
}

#[derive(Debug, Clone, Serialize)]
pub struct PostWritePersistenceReport {
    pub kind: &'static str,
    pub sql_dump_written: bool,
    pub sidecar_auto_persist: Option<SidecarAutoPersistReport>,
}

pub fn preflight_sidecar_post_write(
    project: Option<&Project>,
    namespace: &str,
    _operation: &str,
    effect: Effect,
) -> ExoResult<()> {
    let Some(project) = project else {
        return Ok(());
    };
    if project.policy != StatePolicy::Sidecar {
        return Ok(());
    }
    if !should_auto_persist_after_success(effect, namespace, _operation, Some(project)) {
        return Ok(());
    }
    if !sidecar::sidecar_write_ownership_applies_to_project(project) {
        return Ok(());
    }

    sidecar::ensure_sidecar_write_ownership_for_project(project)
}

pub fn should_persist_after_success(
    project: Option<&Project>,
    namespace: &str,
    operation: &str,
    effect: Effect,
) -> bool {
    should_write_sql_dump(namespace, operation, effect)
        || should_auto_persist_after_success(effect, namespace, operation, project)
}

pub fn persist_after_success(
    workspace_root: &Path,
    project: Option<&Project>,
    namespace: &str,
    operation: &str,
    effect: Effect,
) -> ExoResult<Option<PostWritePersistenceReport>> {
    let should_write_dump = should_write_sql_dump(namespace, operation, effect);
    let should_auto_persist =
        should_auto_persist_after_success(effect, namespace, operation, project);

    if !should_write_dump && !should_auto_persist {
        return Ok(None);
    }

    let mut report = PostWritePersistenceReport {
        kind: "post_write.persistence",
        sql_dump_written: false,
        sidecar_auto_persist: None,
    };

    if should_auto_persist && let Some(project) = project {
        report.sidecar_auto_persist =
            sidecar::checkpoint_after_successful_mutation_with_project(project).map_err(
                |error| sidecar_checkpoint_failure(project, namespace, operation, effect, error),
            )?;
        report.sql_dump_written = true;
    } else if should_write_dump {
        report.sql_dump_written =
            write_sql_dump_after_success(workspace_root, project, namespace, operation, effect)?;
    }

    Ok(Some(report))
}

pub(crate) fn sidecar_checkpoint_failure(
    project: &Project,
    namespace: &str,
    operation: &str,
    effect: Effect,
    error: anyhow::Error,
) -> anyhow::Error {
    let reference = ExoCommandReference::new(&["sidecar", "checkpoint"]);
    anyhow::Error::new(
        ExoFailure::new(
            crate::api::protocol::ErrorCode::PreconditionFailed,
            format!(
                "sidecar local checkpoint failed after durable Exo mutation: {error}"
            ),
            ExoFailure::orienting_steering(vec![SuggestedAction::exo(
                "Retry local sidecar checkpoint",
                reference,
                "Complete the local sidecar checkpoint for this project before continuing sidecar-backed writes.",
                WorkIntent::Execute,
                Some(1.0),
            )]),
        )
        .with_details(serde_json::json!({
            "kind": "sidecar.local_checkpoint",
            "ok": false,
            "mutation_durable_locally": true,
            "checkpoint_complete": false,
            "namespace": namespace,
            "operation": operation,
            "effect": match effect {
                Effect::Pure => "pure",
                Effect::Write => "write",
                Effect::Exec => "exec",
            },
            "sidecar_key": project.sidecar_key.as_deref(),
            "sidecar_root": project.sidecar_root.as_ref(),
            "state_root": &project.state_root,
            "issue": error.to_string(),
        })),
    )
}

pub fn with_sidecar_runtime_lock<T>(project: Option<&Project>, f: impl FnOnce() -> T) -> T {
    let Some(project) = project else {
        return f();
    };
    if project.policy != StatePolicy::Sidecar {
        return f();
    }

    let Some(lock_path) = sidecar_runtime_lock_path(project) else {
        return f();
    };
    let Some(parent) = lock_path.parent() else {
        return f();
    };
    if std::fs::create_dir_all(parent).is_err() {
        return f();
    }

    let Ok(file) = OpenOptions::new()
        .create(true)
        .read(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
    else {
        return f();
    };
    if file.lock_exclusive().is_err() {
        return f();
    }

    let result = f();
    let _ = file.unlock();
    result
}

fn sidecar_runtime_lock_path(project: &Project) -> Option<PathBuf> {
    let sidecar_root = project.sidecar_root.as_ref()?;
    let git_dir = sidecar_root.join(".git");
    if git_dir.is_dir() {
        return Some(git_dir.join("exo-state.lock"));
    }
    Some(project.runtime_dir().join("sidecar-state.lock"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{ProjectId, SidecarAutoPushPolicy};

    fn git_init(root: &Path) {
        let output = std::process::Command::new("git")
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

    fn git_status(root: &Path) -> String {
        let output = std::process::Command::new("git")
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

    fn sidecar_project(workspace: PathBuf, sidecar_root: PathBuf) -> Project {
        Project {
            id: ProjectId::from_git_common_dir(&workspace.join(".git")),
            git_common_dir: workspace.join(".git"),
            workspace_root: Some(workspace),
            policy: StatePolicy::Sidecar,
            projects_config_path: None,
            state_root: sidecar_root.join("projects/demo"),
            sidecar_key: Some("demo".to_string()),
            sidecar_root: Some(sidecar_root),
            sidecar_auto_commit: true,
            sidecar_auto_push: SidecarAutoPushPolicy::IfRemote,
        }
    }

    #[test]
    fn command_event_auto_persist_requires_write_effect() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let sidecar_root = temp.path().join("sidecar");
        let project = sidecar_project(workspace, sidecar_root);

        assert!(!should_auto_persist_after_command_event(
            true,
            Effect::Pure,
            "epoch",
            "add",
            Some(&project)
        ));
        assert!(should_auto_persist_after_command_event(
            true,
            Effect::Write,
            "epoch",
            "add",
            Some(&project)
        ));
        assert!(should_auto_persist_after_command_event(
            true,
            Effect::Exec,
            "strike",
            "start",
            Some(&project)
        ));
        assert!(!should_auto_persist_after_command_event(
            false,
            Effect::Write,
            "epoch",
            "add",
            Some(&project)
        ));
    }

    #[test]
    fn sidecar_exec_effect_requires_post_write_persistence() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let sidecar_root = temp.path().join("sidecar");
        let project = sidecar_project(workspace, sidecar_root);

        assert!(should_persist_after_success(
            Some(&project),
            "strike",
            "start",
            Effect::Exec
        ));
        assert!(should_auto_persist_after_success(
            Effect::Exec,
            "strike",
            "start",
            Some(&project)
        ));
    }

    #[test]
    fn project_resolve_never_requires_sidecar_post_write_persistence() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let sidecar_root = temp.path().join("sidecar");
        let project = sidecar_project(workspace, sidecar_root);

        assert!(!should_persist_after_success(
            Some(&project),
            "project",
            "resolve",
            Effect::Write
        ));
        assert!(!should_auto_persist_after_success(
            Effect::Write,
            "project",
            "resolve",
            Some(&project)
        ));
    }

    #[test]
    fn project_move_root_skips_sidecar_post_write_persistence() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let sidecar_root = temp.path().join("sidecar");
        let project = sidecar_project(workspace, sidecar_root);

        assert!(!should_persist_after_success(
            Some(&project),
            "project",
            "move-root",
            Effect::Exec
        ));
        assert!(!should_auto_persist_after_success(
            Effect::Exec,
            "project",
            "move-root",
            Some(&project)
        ));
    }

    #[test]
    fn storage_maintenance_skips_logical_post_write_persistence() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let sidecar_root = temp.path().join("sidecar");
        let project = sidecar_project(workspace, sidecar_root);

        assert!(!should_write_sql_dump("storage", "maintain", Effect::Exec));
        assert!(!should_persist_after_success(
            Some(&project),
            "storage",
            "maintain",
            Effect::Exec
        ));
        assert!(!should_auto_persist_after_success(
            Effect::Exec,
            "storage",
            "maintain",
            Some(&project)
        ));
        assert!(!should_log_command_event("storage", "maintain"));
    }

    #[test]
    fn current_project_checkpoint_failure_suggests_bare_checkpoint_retry() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let sidecar_root = temp.path().join("sidecar");
        let project = sidecar_project(workspace, sidecar_root);

        let error = sidecar_checkpoint_failure(
            &project,
            "epoch",
            "add",
            Effect::Write,
            anyhow::anyhow!("projection failed"),
        );
        let failure = error
            .downcast_ref::<ExoFailure>()
            .expect("checkpoint failure remains structured");

        assert!(
            failure
                .steering
                .next_actions
                .iter()
                .any(|action| action.command == "exo sidecar checkpoint"),
            "{failure:?}"
        );
        assert!(
            failure
                .steering
                .next_actions
                .iter()
                .all(|action| !action.command.contains("--project")),
            "{failure:?}"
        );
    }

    #[test]
    fn sidecar_runtime_lock_uses_git_dir_without_dirtying_sidecar_repo() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path().join("workspace");
        let sidecar_root = temp.path().join("sidecar");
        std::fs::create_dir_all(&workspace).expect("create workspace");
        std::fs::create_dir_all(&sidecar_root).expect("create sidecar root");
        git_init(&sidecar_root);

        let project = sidecar_project(workspace, sidecar_root.clone());

        with_sidecar_runtime_lock(Some(&project), || {});

        assert_eq!(git_status(&sidecar_root), "");
        assert!(sidecar_root.join(".git/exo-state.lock").exists());
    }
}
