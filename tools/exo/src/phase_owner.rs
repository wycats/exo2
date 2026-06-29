//! Worktree-aware phase ownership.
//!
//! `workspace_active_phase` is a focus pointer: it says what this workspace is
//! looking at. Phase ownership is a separate claim that controls who may mutate
//! phase-scoped state.

use crate::api::protocol::ErrorCode;
use crate::context::sqlite_loader::PhaseOwnerData;
use crate::context::{SqliteLoader, SqliteWriter};
use crate::failure::ExoFailure;
use crate::project::Project;
use crate::steering::{SuggestedAction, WorkIntent};
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurrentPhaseOwner {
    pub owner_kind: String,
    pub owner_id: String,
    pub owner_basis: String,
    pub workspace_id: String,
    pub workspace_root: Option<String>,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CurrentOwnerView {
    pub owner_kind: String,
    pub owner_id: String,
    pub owner_basis: String,
    pub workspace_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseOwnerView {
    pub owner_kind: String,
    pub owner_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_by_workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_by_workspace_root: Option<String>,
    pub claimed_at: String,
    pub updated_at: String,
    pub owned_here: bool,
    pub owned_elsewhere: bool,
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_reason: Option<String>,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PhaseOwnershipTransition {
    pub owner: PhaseOwnerView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_owner: Option<PhaseOwnerView>,
    pub took_over: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PhaseOwnerViewContext {
    current: CurrentPhaseOwner,
    local_branches: Option<HashSet<String>>,
    worktrees: Option<HashMap<PathBuf, bool>>,
}

impl PhaseOwnerViewContext {
    pub(crate) fn new(root: &Path, project: Option<&Project>) -> Self {
        Self {
            current: current_owner(root, project),
            local_branches: local_branch_names(root),
            worktrees: worktree_index(root),
        }
    }

    pub(crate) fn current_owner(&self) -> &CurrentPhaseOwner {
        &self.current
    }

    pub(crate) fn owner_view(&self, owner: &PhaseOwnerData) -> PhaseOwnerView {
        let owned_here = owner_matches(owner, &self.current);
        let stale_reason = self.stale_reason(owner);
        let stale = stale_reason.is_some();
        let label = owner_label(owner);

        PhaseOwnerView {
            owner_kind: owner.owner_kind.clone(),
            owner_id: owner.owner_id.clone(),
            claimed_by_workspace_id: owner.claimed_by_workspace_id.clone(),
            claimed_by_workspace_root: owner.claimed_by_workspace_root.clone(),
            claimed_at: owner.claimed_at.clone(),
            updated_at: owner.updated_at.clone(),
            owned_here,
            owned_elsewhere: !owned_here,
            stale,
            stale_reason,
            label,
        }
    }

    fn stale_reason(&self, owner: &PhaseOwnerData) -> Option<String> {
        match owner.owner_kind.as_str() {
            "workspace" => self.workspace_stale_reason(owner.claimed_by_workspace_root.as_deref()),
            "branch" => self.branch_stale_reason(&owner.owner_id),
            _ => None,
        }
    }

    fn workspace_stale_reason(&self, workspace_root: Option<&str>) -> Option<String> {
        let workspace_root = workspace_root?;
        let workspace_path = PathBuf::from(workspace_root);
        if !workspace_path.exists() {
            return Some("workspace root is missing".to_string());
        }

        let claimed = canonical_or_original(&workspace_path);
        let Some(worktrees) = self.worktrees.as_ref() else {
            return None;
        };

        match worktrees.get(&claimed) {
            Some(true) => Some("workspace root is prunable".to_string()),
            Some(false) => None,
            None => Some("workspace root is not a registered git worktree".to_string()),
        }
    }

    fn branch_stale_reason(&self, branch: &str) -> Option<String> {
        let branches = self.local_branches.as_ref()?;
        (!branches.contains(branch)).then_some("branch is missing locally".to_string())
    }
}

pub(crate) fn current_owner(root: &Path, project: Option<&Project>) -> CurrentPhaseOwner {
    let workspace_root = workspace_root_text(root, project);
    let project_id = project.map_or_else(|| "unresolved".to_string(), |p| p.id.to_string());
    let branch = current_branch(root);
    derive_current_owner(&project_id, workspace_root, branch)
}

pub(crate) fn current_owner_view(root: &Path, project: Option<&Project>) -> CurrentOwnerView {
    current_owner(root, project).into()
}

pub(crate) fn current_owner_basis_label(owner: &CurrentOwnerView) -> String {
    match owner.owner_basis.as_str() {
        "branch" => owner
            .branch
            .as_deref()
            .map_or_else(|| "branch".to_string(), |branch| format!("branch {branch}")),
        "codex_workspace" => "codex workspace".to_string(),
        "detached_workspace" => "detached workspace".to_string(),
        "unresolved_workspace" => "unresolved workspace".to_string(),
        other => other.replace('_', " "),
    }
}

impl From<CurrentPhaseOwner> for CurrentOwnerView {
    fn from(owner: CurrentPhaseOwner) -> Self {
        Self {
            owner_kind: owner.owner_kind,
            owner_id: owner.owner_id,
            owner_basis: owner.owner_basis,
            workspace_id: owner.workspace_id,
            workspace_root: owner.workspace_root,
            branch: owner.branch,
        }
    }
}

fn derive_current_owner(
    project_id: &str,
    workspace_root: Option<String>,
    branch: Option<String>,
) -> CurrentPhaseOwner {
    let workspace_id = workspace_id(project_id, workspace_root.as_deref().unwrap_or(""));
    let codex_worktree = is_codex_worktree(workspace_root.as_deref().unwrap_or(""));

    if let Some(branch_name) = branch.as_ref()
        && !codex_worktree
    {
        return CurrentPhaseOwner {
            owner_kind: "branch".to_string(),
            owner_id: branch_name.clone(),
            owner_basis: "branch".to_string(),
            workspace_id,
            workspace_root,
            branch,
        };
    }

    let owner_basis = if codex_worktree {
        "codex_workspace"
    } else if workspace_root.is_some() {
        "detached_workspace"
    } else {
        "unresolved_workspace"
    };

    CurrentPhaseOwner {
        owner_kind: "workspace".to_string(),
        owner_id: workspace_id.clone(),
        owner_basis: owner_basis.to_string(),
        workspace_id,
        workspace_root,
        branch,
    }
}

pub(crate) fn owner_view_for_phase(
    root: &Path,
    project: Option<&Project>,
    db_path: &Path,
    phase_id: &str,
) -> Result<Option<PhaseOwnerView>> {
    let loader = SqliteLoader::open(db_path)?;
    let Some(owner) = loader.load_phase_owner(phase_id)? else {
        return Ok(None);
    };
    let view_context = PhaseOwnerViewContext::new(root, project);
    Ok(Some(view_context.owner_view(&owner)))
}

pub(crate) fn claim_phase_for_current_owner(
    root: &Path,
    project: Option<&Project>,
    db_path: &Path,
    phase_id: &str,
    take_over: bool,
) -> Result<PhaseOwnershipTransition> {
    let loader = SqliteLoader::open(db_path)?;
    let view_context = PhaseOwnerViewContext::new(root, project);
    let current = view_context.current_owner();
    let previous = loader.load_phase_owner(phase_id)?;

    if let Some(previous_owner) = previous.as_ref() {
        let previous_view = view_context.owner_view(previous_owner);
        if !owner_matches(previous_owner, current) && !take_over {
            return Err(anyhow::Error::new(ownership_conflict_failure(
                phase_id,
                &previous_view,
            )));
        }
    }

    let writer = SqliteWriter::open(db_path)?;
    let claimed = writer.claim_phase_owner_if_current(
        phase_id,
        &current.owner_kind,
        &current.owner_id,
        Some(&current.workspace_id),
        current.workspace_root.as_deref(),
        previous
            .as_ref()
            .map(|owner| (owner.owner_kind.as_str(), owner.owner_id.as_str())),
    )?;

    let loader = SqliteLoader::open(db_path)?;
    if !claimed {
        match loader.load_phase_owner(phase_id)? {
            Some(owner) if owner_matches(&owner, current) => {}
            Some(owner) => {
                let owner = view_context.owner_view(&owner);
                return Err(anyhow::Error::new(ownership_conflict_failure(
                    phase_id, &owner,
                )));
            }
            None => {
                return Err(anyhow::Error::new(ownership_changed_failure(phase_id)));
            }
        }
    }

    let owner = loader
        .load_phase_owner(phase_id)?
        .context("phase owner missing immediately after claim")?;
    let owner = view_context.owner_view(&owner);
    let previous_owner = previous.map(|record| view_context.owner_view(&record));
    let took_over = previous_owner.as_ref().is_some_and(|previous| {
        previous.owner_kind != owner.owner_kind || previous.owner_id != owner.owner_id
    });

    Ok(PhaseOwnershipTransition {
        owner,
        previous_owner,
        took_over,
    })
}

pub(crate) fn ensure_phase_write_allowed(
    root: &Path,
    project: Option<&Project>,
    db_path: &Path,
    phase_id: &str,
) -> Result<()> {
    let loader = SqliteLoader::open(db_path)?;
    let view_context = PhaseOwnerViewContext::new(root, project);
    let Some(owner) = loader.load_phase_owner(phase_id)? else {
        let current = view_context.current_owner();
        let writer = SqliteWriter::open(db_path)?;
        let claimed = writer.claim_phase_owner_if_current(
            phase_id,
            &current.owner_kind,
            &current.owner_id,
            Some(&current.workspace_id),
            current.workspace_root.as_deref(),
            None,
        )?;
        if claimed {
            return Ok(());
        }

        let loader = SqliteLoader::open(db_path)?;
        let Some(owner) = loader.load_phase_owner(phase_id)? else {
            return Err(anyhow::Error::new(ownership_changed_failure(phase_id)));
        };
        if owner_matches(&owner, current) {
            return Ok(());
        }

        let owner = view_context.owner_view(&owner);
        return Err(anyhow::Error::new(ownership_conflict_failure(
            phase_id, &owner,
        )));
    };

    if owner_matches(&owner, view_context.current_owner()) {
        return Ok(());
    }

    let owner = view_context.owner_view(&owner);
    Err(anyhow::Error::new(ownership_conflict_failure(
        phase_id, &owner,
    )))
}

pub(crate) fn release_phase_owner(
    root: &Path,
    project: Option<&Project>,
    db_path: &Path,
    phase_id: &str,
) -> Result<Option<PhaseOwnerView>> {
    let loader = SqliteLoader::open(db_path)?;
    let Some(owner) = loader.load_phase_owner(phase_id)? else {
        return Ok(None);
    };

    let view_context = PhaseOwnerViewContext::new(root, project);
    let view = view_context.owner_view(&owner);
    if !owner_matches(&owner, view_context.current_owner()) && !view.stale {
        return Err(anyhow::Error::new(ownership_conflict_failure(
            phase_id, &view,
        )));
    }

    let writer = SqliteWriter::open(db_path)?;
    let cleared =
        writer.clear_phase_owner_if_current(phase_id, &owner.owner_kind, &owner.owner_id)?;
    if !cleared {
        return Err(anyhow::Error::new(ownership_changed_failure(phase_id)));
    }
    Ok(Some(view))
}

fn owner_matches(owner: &PhaseOwnerData, current: &CurrentPhaseOwner) -> bool {
    owner.owner_kind == current.owner_kind && owner.owner_id == current.owner_id
}

fn ownership_conflict_failure(phase_id: &str, owner: &PhaseOwnerView) -> ExoFailure {
    let mut actions = vec![
        SuggestedAction {
            label: "Inspect phase".to_string(),
            command: format!("exo phase read-details {phase_id}"),
            rationale: "Read the phase without changing its owner.".to_string(),
            intent: WorkIntent::Orient,
            confidence: Some(0.9),
        },
        SuggestedAction {
            label: "Focus read-only".to_string(),
            command: format!("exo phase focus {phase_id}"),
            rationale: "Point this workspace at the phase without taking its mutation claim."
                .to_string(),
            intent: WorkIntent::Orient,
            confidence: Some(0.8),
        },
    ];

    if owner.stale {
        actions.push(SuggestedAction {
            label: "Release stale owner".to_string(),
            command: format!("exo phase release {phase_id}"),
            rationale: "Clear the stale phase owner before claiming the phase.".to_string(),
            intent: WorkIntent::Execute,
            confidence: Some(0.8),
        });
    }

    actions.push(SuggestedAction {
        label: "Take over phase".to_string(),
        command: format!("exo phase start {phase_id} --take-over"),
        rationale: "Explicitly replace the current phase owner with this workspace's owner."
            .to_string(),
        intent: WorkIntent::Execute,
        confidence: Some(0.5),
    });

    ExoFailure::new(
        ErrorCode::PreconditionFailed,
        format!(
            "Phase '{phase_id}' is owned by another {} ({})",
            owner.owner_kind, owner.label
        ),
        ExoFailure::orienting_steering(actions),
    )
    .with_details(serde_json::json!({
        "phaseId": phase_id,
        "owner": owner,
    }))
}

fn ownership_changed_failure(phase_id: &str) -> ExoFailure {
    ExoFailure::new(
        ErrorCode::PreconditionFailed,
        format!("Phase '{phase_id}' ownership changed while claiming it"),
        ExoFailure::orienting_steering(vec![SuggestedAction {
            label: "Retry phase start".to_string(),
            command: format!("exo phase start {phase_id}"),
            rationale: "Reload the current phase owner and retry the claim.".to_string(),
            intent: WorkIntent::Execute,
            confidence: Some(0.7),
        }]),
    )
    .with_details(serde_json::json!({
        "phaseId": phase_id,
    }))
}

fn owner_label(owner: &PhaseOwnerData) -> String {
    match owner.owner_kind.as_str() {
        "branch" => format!("branch {}", owner.owner_id),
        "workspace" => owner
            .claimed_by_workspace_root
            .as_deref()
            .and_then(|root| Path::new(root).file_name())
            .and_then(|name| name.to_str())
            .map_or_else(
                || format!("workspace {}", short_id(&owner.owner_id)),
                |name| format!("workspace {name}"),
            ),
        "pr" => format!("PR {}", owner.owner_id),
        _ => format!("{} {}", owner.owner_kind, owner.owner_id),
    }
}

fn worktree_index(repo_root: &Path) -> Option<HashMap<PathBuf, bool>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    Some(parse_worktree_index(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

fn parse_worktree_index(output: &str) -> HashMap<PathBuf, bool> {
    let mut worktrees = HashMap::new();
    let mut block_path: Option<PathBuf> = None;
    let mut block_prunable = false;

    for line in output.lines().chain([""]) {
        if line.is_empty() {
            if let Some(path) = block_path.take() {
                worktrees.insert(canonical_or_original(&path), block_prunable);
            }
            block_prunable = false;
            continue;
        }

        if let Some(path) = line.strip_prefix("worktree ") {
            block_path = Some(PathBuf::from(path));
        } else if line.starts_with("prunable") {
            block_prunable = true;
        }
    }

    worktrees
}

fn local_branch_names(repo_root: &Path) -> Option<HashSet<String>> {
    let output = Command::new("git")
        .args(["for-each-ref", "--format=%(refname:short)", "refs/heads"])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return Some(HashSet::new());
    }

    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|branch| !branch.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
    )
}

fn workspace_root_text(root: &Path, project: Option<&Project>) -> Option<String> {
    project
        .and_then(|project| project.workspace_root.as_ref())
        .cloned()
        .or_else(|| root.canonicalize().ok())
        .or_else(|| Some(root.to_path_buf()))
        .map(|root| root.to_string_lossy().into_owned())
}

fn workspace_id(project_id: &str, workspace_root: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(project_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(workspace_root.as_bytes());
    let hash = hasher.finalize().to_hex();
    format!("workspace:{project_id}:{}", &hash.as_str()[..16])
}

fn short_id(id: &str) -> &str {
    id.rsplit(':')
        .next()
        .map_or(id, |suffix| suffix.get(..8).unwrap_or(suffix))
}

fn current_branch(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!branch.is_empty()).then_some(branch)
}

fn is_codex_worktree(workspace_root: &str) -> bool {
    workspace_root
        .replace('\\', "/")
        .contains("/.codex/worktrees/")
}

fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_worktree_detection_handles_windows_separators() {
        assert!(is_codex_worktree(
            r"C:\Users\dev\.codex\worktrees\155b\exo2"
        ));
        assert!(is_codex_worktree("/Users/dev/.codex/worktrees/155b/exo2"));
        assert!(!is_codex_worktree(r"C:\Users\dev\Code\exo2"));
    }

    #[test]
    fn owner_derivation_reports_branch_basis_for_named_non_codex_worktree() {
        let owner = derive_current_owner(
            "project",
            Some("/Users/dev/Code/exo2".to_string()),
            Some("feature/phase-owner".to_string()),
        );
        assert_eq!(owner.owner_kind, "branch");
        assert_eq!(owner.owner_id, "feature/phase-owner");
        assert_eq!(owner.owner_basis, "branch");
        assert_eq!(owner.branch.as_deref(), Some("feature/phase-owner"));
    }

    #[test]
    fn owner_derivation_prefers_codex_workspace_basis_over_branch() {
        let owner = derive_current_owner(
            "project",
            Some(r"C:\Users\dev\.codex\worktrees\155b\exo2".to_string()),
            Some("feature/phase-owner".to_string()),
        );
        assert_eq!(owner.owner_kind, "workspace");
        assert!(owner.owner_id.starts_with("workspace:project:"));
        assert_eq!(owner.owner_basis, "codex_workspace");
        assert_eq!(owner.branch.as_deref(), Some("feature/phase-owner"));
    }

    #[test]
    fn owner_derivation_reports_detached_workspace_basis() {
        let owner = derive_current_owner("project", Some("/Users/dev/Code/exo2".to_string()), None);
        assert_eq!(owner.owner_kind, "workspace");
        assert!(owner.owner_id.starts_with("workspace:project:"));
        assert_eq!(owner.owner_basis, "detached_workspace");
        assert!(owner.branch.is_none());
    }

    #[test]
    fn owner_derivation_reports_unresolved_workspace_basis() {
        let owner = derive_current_owner("unresolved", None, None);
        assert_eq!(owner.owner_kind, "workspace");
        assert!(owner.owner_id.starts_with("workspace:unresolved:"));
        assert_eq!(owner.owner_basis, "unresolved_workspace");
        assert!(owner.workspace_root.is_none());
        assert!(owner.branch.is_none());
    }

    #[test]
    fn worktree_parser_marks_prunable_records_with_reason() {
        let output = "\
worktree /tmp/exo-live
HEAD 1111111111111111111111111111111111111111
branch refs/heads/main

worktree /tmp/exo-broken
HEAD 2222222222222222222222222222222222222222
prunable gitdir file points to non-existent location

";
        let worktrees = parse_worktree_index(output);
        assert_eq!(
            worktrees.get(&canonical_or_original(Path::new("/tmp/exo-live"))),
            Some(&false)
        );
        assert_eq!(
            worktrees.get(&canonical_or_original(Path::new("/tmp/exo-broken"))),
            Some(&true)
        );
    }
}
