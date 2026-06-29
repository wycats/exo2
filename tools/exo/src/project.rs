use crate::ExoResult;
use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use toml_edit::{DocumentMut, Item, value};

const PROJECTS_CONFIG_RELATIVE_PATH: &str = "exo/projects.toml";
const DEFAULT_SIDECAR_ROOT_RELATIVE_PATH: &str = "exo/sidecars";
const SIDECAR_RUNTIME_GITIGNORE_ENTRIES: &[&str] = &["projects/*/cache/", "projects/*/runtime/"];
pub(crate) const SIDECAR_GIT_USER_NAME: &str = "Exosuit";
pub(crate) const SIDECAR_GIT_USER_EMAIL: &str = "exo@exosuit.local";
pub const MAX_PORTABLE_UNIX_SOCKET_PATH_LEN: usize = 100;

type ResolvedStateRoot = (
    StatePolicy,
    PathBuf,
    Option<String>,
    Option<PathBuf>,
    bool,
    SidecarAutoPushPolicy,
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: ProjectId,
    pub git_common_dir: PathBuf,
    pub workspace_root: Option<PathBuf>,
    pub policy: StatePolicy,
    pub projects_config_path: Option<PathBuf>,
    pub state_root: PathBuf,
    pub sidecar_key: Option<String>,
    pub sidecar_root: Option<PathBuf>,
    pub sidecar_auto_commit: bool,
    pub sidecar_auto_push: SidecarAutoPushPolicy,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProjectCatalog {
    pub current_project_id: Option<String>,
    pub projects: Vec<ProjectCatalogEntry>,
    pub diagnostics: Vec<ProjectCatalogDiagnostic>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProjectCatalogEntry {
    pub id: String,
    pub display_name: String,
    pub source: &'static str,
    pub state: &'static str,
    pub workspace_root: Option<PathBuf>,
    pub git_common_dir: Option<PathBuf>,
    pub state_root: Option<PathBuf>,
    pub db_path: Option<PathBuf>,
    pub runtime_dir: Option<PathBuf>,
    pub sidecar_key: Option<String>,
    pub sidecar_root: Option<PathBuf>,
    pub state_readable: bool,
    pub workspace_available: bool,
    pub commands_available: bool,
    pub write_available: bool,
    pub selectable: bool,
    pub current: bool,
    pub diagnostics: Vec<ProjectCatalogDiagnostic>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProjectCatalogDiagnostic {
    pub source: String,
    pub severity: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProjectPolicyRepairPlan {
    pub config_path: PathBuf,
    pub stale_sidecars: Vec<StaleSidecarPolicyEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProjectPolicyRepairApply {
    pub config_path: PathBuf,
    pub removed: Vec<StaleSidecarPolicyEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StaleSidecarPolicyEntry {
    pub id: String,
    pub sidecar_key: String,
    pub sidecar_root: PathBuf,
    pub state_root: PathBuf,
    pub reason: String,
}

impl Project {
    pub fn resolve(cwd: &Path) -> ExoResult<Self> {
        ProjectResolver::default().resolve(cwd)
    }

    pub fn db_path(&self) -> PathBuf {
        project_db_path(&self.state_root)
    }

    pub fn runtime_dir(&self) -> PathBuf {
        self.state_root.join("runtime")
    }

    pub fn socket_path(&self) -> PathBuf {
        daemon_socket_path_for_runtime(&self.runtime_dir())
    }

    pub fn pid_path(&self) -> PathBuf {
        self.runtime_dir().join("daemon.pid")
    }

    pub fn sidecar_project_dir(&self) -> Option<PathBuf> {
        Some(
            self.sidecar_root
                .as_ref()?
                .join("projects")
                .join(self.sidecar_key.as_ref()?),
        )
    }

    pub fn sidecar_manifest_path(&self) -> Option<PathBuf> {
        Some(self.sidecar_project_dir()?.join("sidecar.toml"))
    }

    pub fn sidecar_projection_dir(&self) -> Option<PathBuf> {
        Some(self.sidecar_project_dir()?.join("agent-context"))
    }
}

fn project_db_path(state_root: &Path) -> PathBuf {
    state_root.join("cache").join("exo.db")
}

pub fn daemon_socket_path_for_runtime(runtime_dir: &Path) -> PathBuf {
    let default = runtime_dir.join("daemon.sock");
    if default.to_string_lossy().len() < MAX_PORTABLE_UNIX_SOCKET_PATH_LEN {
        return default;
    }

    let digest = blake3::hash(runtime_dir.to_string_lossy().as_bytes());
    let socket_name = format!("exo-{}.sock", &digest.to_hex()[..32]);
    PathBuf::from("/tmp")
        .join("exo-daemon-sockets")
        .join(socket_name)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectId(String);

impl ProjectId {
    pub fn from_git_common_dir(git_common_dir: &Path) -> Self {
        let bytes = git_common_dir.as_os_str().as_encoded_bytes();
        let hash = blake3::hash(bytes);
        let mut id = String::with_capacity(16);
        for byte in &hash.as_bytes()[..8] {
            id.push_str(&format!("{byte:02x}"));
        }
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatePolicy {
    Repo,
    Shadow,
    Sidecar,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SidecarAutoPushPolicy {
    Never,
    #[default]
    IfRemote,
    Always,
}

impl SidecarAutoPushPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::IfRemote => "if_remote",
            Self::Always => "always",
        }
    }

    fn parse(value: &str) -> ExoResult<Self> {
        match value {
            "never" => Ok(Self::Never),
            "if_remote" | "if-remote" => Ok(Self::IfRemote),
            "always" => Ok(Self::Always),
            other => anyhow::bail!(
                "Invalid sidecar auto_push policy {other:?}; use never, if_remote, or always"
            ),
        }
    }
}

impl StatePolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Repo => "repo",
            Self::Shadow => "shadow",
            Self::Sidecar => "sidecar",
        }
    }
}

impl std::fmt::Display for ProjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProjectResolver {
    home_dir: Option<PathBuf>,
    config_home: Option<PathBuf>,
}

impl ProjectResolver {
    pub fn with_home_dir(mut self, home_dir: impl Into<PathBuf>) -> Self {
        self.home_dir = Some(home_dir.into());
        self
    }

    pub fn with_config_home(mut self, config_home: impl Into<PathBuf>) -> Self {
        self.config_home = Some(config_home.into());
        self
    }

    pub fn with_projects_config_path(mut self, projects_config_path: &Path) -> Self {
        if let Some(config_home) = projects_config_path
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
        {
            self.config_home = Some(config_home);
        }
        self
    }

    pub fn resolve(&self, cwd: &Path) -> ExoResult<Project> {
        let git_common_dir = self.resolve_git_common_dir(cwd)?;
        let workspace_root = self.resolve_workspace_root(cwd)?;
        let id = ProjectId::from_git_common_dir(&git_common_dir);
        let projects_config_path = self.projects_config_path();
        let (policy, state_root, sidecar_key, sidecar_root, sidecar_auto_commit, sidecar_auto_push) =
            self.resolve_state_root(&id, &git_common_dir)?;

        Ok(Project {
            id,
            git_common_dir,
            workspace_root,
            policy,
            projects_config_path,
            state_root,
            sidecar_key,
            sidecar_root,
            sidecar_auto_commit,
            sidecar_auto_push,
        })
    }

    pub fn list_catalog(&self, cwd: &Path) -> ExoResult<ProjectCatalog> {
        let mut entries: BTreeMap<String, ProjectCatalogEntry> = BTreeMap::new();
        let mut diagnostics = Vec::new();
        let current_project = match self.resolve(cwd) {
            Ok(project) => Some(project),
            Err(error) => {
                diagnostics.push(ProjectCatalogDiagnostic {
                    source: "current".to_string(),
                    severity: "warning",
                    message: error.to_string(),
                });
                None
            }
        };
        let current_project_id = current_project
            .as_ref()
            .map(|project| project.id.as_str().to_string());

        if let Some(project) = current_project.as_ref() {
            upsert_catalog_entry(
                &mut entries,
                ProjectCatalogEntry::from_resolved_project(project, "current", true),
            );
        }

        let policy = match self.load_projects_policy() {
            Ok(Some(policy)) => Some(policy),
            Ok(None) => None,
            Err(error) => {
                diagnostics.push(ProjectCatalogDiagnostic {
                    source: "local-policy".to_string(),
                    severity: "warning",
                    message: error.to_string(),
                });
                None
            }
        };

        if let Some(policy) = &policy {
            let shadow_projects_root = self
                .home_dir()
                .ok()
                .map(|home| home.join(".exo").join("projects"));
            for (id, project) in &policy.projects {
                upsert_catalog_entry(
                    &mut entries,
                    ProjectCatalogEntry::from_policy(
                        id,
                        project,
                        current_project_id.as_deref(),
                        shadow_projects_root.as_deref(),
                    ),
                );
            }
        }

        let sidecar_roots = self.catalog_sidecar_roots(policy.as_ref())?;
        for sidecar_root in sidecar_roots {
            match self.scan_sidecar_root(&sidecar_root, current_project_id.as_deref()) {
                Ok(sidecar_entries) => {
                    for entry in sidecar_entries {
                        upsert_catalog_entry(&mut entries, entry);
                    }
                }
                Err(error) => diagnostics.push(ProjectCatalogDiagnostic {
                    source: format!("sidecar-root:{}", sidecar_root.display()),
                    severity: "warning",
                    message: error.to_string(),
                }),
            }
        }

        let mut projects = Vec::new();
        for entry in entries.into_values() {
            if entry.is_stale_local_policy_sidecar() && !entry.current {
                diagnostics.extend(entry.diagnostics);
                continue;
            }
            projects.push(entry);
        }
        projects.sort_by(|left, right| {
            right
                .current
                .cmp(&left.current)
                .then_with(|| right.selectable.cmp(&left.selectable))
                .then_with(|| left.display_name.cmp(&right.display_name))
                .then_with(|| left.id.cmp(&right.id))
        });

        Ok(ProjectCatalog {
            current_project_id,
            projects,
            diagnostics,
        })
    }

    fn resolve_git_common_dir(&self, cwd: &Path) -> ExoResult<PathBuf> {
        let output = run_git(
            cwd,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;

        if !output.status.success() {
            if git_required(&output) {
                return Err(anyhow!(
                    "exo requires a git repository. Open a git-backed project or run `git init` first."
                ));
            }

            return Err(git_command_error(
                cwd,
                "git rev-parse --path-format=absolute --git-common-dir",
                &output,
            ));
        }

        let path = git_stdout_path(&output).ok_or_else(|| {
            anyhow!(
                "git did not report a common git directory for {}",
                cwd.display()
            )
        })?;

        path.canonicalize().with_context(|| {
            format!(
                "Failed to canonicalize git common directory {}",
                path.display()
            )
        })
    }

    fn resolve_workspace_root(&self, cwd: &Path) -> ExoResult<Option<PathBuf>> {
        let output = run_git(
            cwd,
            &["rev-parse", "--path-format=absolute", "--show-toplevel"],
        )?;

        if output.status.success() {
            let Some(path) = git_stdout_path(&output) else {
                return Ok(None);
            };
            return path.canonicalize().map(Some).with_context(|| {
                format!("Failed to canonicalize workspace root {}", path.display())
            });
        }

        let bare_output = run_git(cwd, &["rev-parse", "--is-bare-repository"])?;
        if bare_output.status.success()
            && String::from_utf8_lossy(&bare_output.stdout).trim() == "true"
        {
            return Ok(None);
        }

        Err(git_command_error(
            cwd,
            "git rev-parse --path-format=absolute --show-toplevel",
            &output,
        ))
    }

    fn resolve_state_root(
        &self,
        id: &ProjectId,
        git_common_dir: &Path,
    ) -> ExoResult<ResolvedStateRoot> {
        if let Some(binding) = self.sidecar_binding(id)? {
            let state_root = binding.root.join("projects").join(&binding.key);
            return Ok((
                StatePolicy::Sidecar,
                state_root,
                Some(binding.key),
                Some(binding.root),
                binding.auto_commit,
                binding.auto_push,
            ));
        }

        if self.shadow_enabled(id)? {
            let home = self.home_dir()?;
            return Ok((
                StatePolicy::Shadow,
                home.join(".exo").join("projects").join(id.as_str()),
                None,
                None,
                false,
                SidecarAutoPushPolicy::Never,
            ));
        }

        let parent = git_common_dir.parent().ok_or_else(|| {
            anyhow!(
                "Git common directory has no parent: {}",
                git_common_dir.display()
            )
        })?;

        Ok((
            StatePolicy::Repo,
            parent.join(".exo"),
            None,
            None,
            false,
            SidecarAutoPushPolicy::Never,
        ))
    }

    fn sidecar_binding(&self, id: &ProjectId) -> ExoResult<Option<SidecarBinding>> {
        let Some(path) = self.projects_config_path() else {
            return Ok(None);
        };

        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read project policy at {}", path.display()))?;
        let policy: ProjectsPolicy = toml::from_str(&content)
            .with_context(|| format!("Failed to parse project policy at {}", path.display()))?;

        let Some(project) = policy.projects.get(id.as_str()) else {
            return Ok(None);
        };

        if project.state.as_deref() != Some("sidecar") {
            return Ok(None);
        }

        let key = project
            .sidecar_key
            .clone()
            .ok_or_else(|| anyhow!("Sidecar policy for project {id} is missing sidecar_key"))?;
        let root = project
            .sidecar_root
            .as_ref()
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("Sidecar policy for project {id} is missing sidecar_root"))?;

        let auto_commit = project.auto_commit.unwrap_or(true);
        let auto_push = project
            .auto_push
            .as_deref()
            .map(SidecarAutoPushPolicy::parse)
            .transpose()?
            .unwrap_or_default();

        Ok(Some(SidecarBinding {
            key,
            root,
            auto_commit,
            auto_push,
        }))
    }

    fn shadow_enabled(&self, id: &ProjectId) -> ExoResult<bool> {
        let Some(path) = self.projects_config_path() else {
            return Ok(false);
        };

        if !path.exists() {
            return Ok(false);
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read project policy at {}", path.display()))?;
        let policy: ProjectsPolicy = toml::from_str(&content)
            .with_context(|| format!("Failed to parse project policy at {}", path.display()))?;

        Ok(policy
            .projects
            .get(id.as_str())
            .is_some_and(ProjectPolicy::uses_shadow_state))
    }

    fn projects_config_path(&self) -> Option<PathBuf> {
        config_home_from(self.config_home.clone(), self.home_dir.clone(), |name| {
            std::env::var_os(name)
        })
        .map(|config_home| config_home.join(PROJECTS_CONFIG_RELATIVE_PATH))
    }

    fn load_projects_policy(&self) -> ExoResult<Option<ProjectsPolicy>> {
        let Some(path) = self.projects_config_path() else {
            return Ok(None);
        };

        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read project policy at {}", path.display()))?;
        let policy: ProjectsPolicy = toml::from_str(&content)
            .with_context(|| format!("Failed to parse project policy at {}", path.display()))?;
        Ok(Some(policy))
    }

    fn catalog_sidecar_roots(
        &self,
        policy: Option<&ProjectsPolicy>,
    ) -> ExoResult<BTreeSet<PathBuf>> {
        let mut roots = BTreeSet::new();

        if let Some(policy) = policy {
            for project in policy.projects.values() {
                if let Some(root) = &project.sidecar_root {
                    roots.insert(PathBuf::from(root));
                }
            }
        }

        if let Ok(default_root) = self.default_sidecar_root() {
            if default_root.exists() {
                roots.insert(default_root);
            }
        }

        Ok(roots)
    }

    fn scan_sidecar_root(
        &self,
        sidecar_root: &Path,
        current_project_id: Option<&str>,
    ) -> ExoResult<Vec<ProjectCatalogEntry>> {
        let projects_dir = sidecar_root.join("projects");
        if !projects_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&projects_dir).with_context(|| {
            format!(
                "Failed to read sidecar projects directory {}",
                projects_dir.display()
            )
        })? {
            let entry = entry.with_context(|| {
                format!(
                    "Failed to read sidecar project entry in {}",
                    projects_dir.display()
                )
            })?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            entries.push(ProjectCatalogEntry::from_sidecar_project_dir(
                sidecar_root,
                &path,
                current_project_id,
            ));
        }
        Ok(entries)
    }

    pub fn local_projects_config_path(&self) -> ExoResult<PathBuf> {
        self.projects_config_path().ok_or_else(|| {
            anyhow!("Project policy requires XDG_CONFIG_HOME or a home/config directory to be set")
        })
    }

    pub fn default_sidecar_root(&self) -> ExoResult<PathBuf> {
        let home = self
            .home_dir()
            .map_err(|_| anyhow!("Default sidecar root requires a home directory to be set"))?;
        Ok(home.join(DEFAULT_SIDECAR_ROOT_RELATIVE_PATH))
    }

    pub fn preview_stale_sidecar_policy_repair(&self) -> ExoResult<ProjectPolicyRepairPlan> {
        let config_path = self.local_projects_config_path()?;
        let stale_sidecars = self.stale_sidecar_policy_entries(&config_path)?;
        Ok(ProjectPolicyRepairPlan {
            config_path,
            stale_sidecars,
        })
    }

    pub fn apply_stale_sidecar_policy_repair(&self) -> ExoResult<ProjectPolicyRepairApply> {
        let plan = self.preview_stale_sidecar_policy_repair()?;
        if plan.stale_sidecars.is_empty() {
            return Ok(ProjectPolicyRepairApply {
                config_path: plan.config_path,
                removed: Vec::new(),
            });
        }

        let mut doc = read_projects_doc(&plan.config_path)?;
        let Some(projects) = doc["projects"].as_table_mut() else {
            return Ok(ProjectPolicyRepairApply {
                config_path: plan.config_path,
                removed: Vec::new(),
            });
        };

        let mut removed = Vec::new();
        for entry in plan.stale_sidecars {
            if projects.remove(&entry.id).is_some() {
                removed.push(entry);
            }
        }
        if !removed.is_empty() {
            write_projects_doc(&plan.config_path, &doc)?;
        }

        Ok(ProjectPolicyRepairApply {
            config_path: plan.config_path,
            removed,
        })
    }

    fn stale_sidecar_policy_entries(
        &self,
        config_path: &Path,
    ) -> ExoResult<Vec<StaleSidecarPolicyEntry>> {
        if !config_path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(config_path).with_context(|| {
            format!("Failed to read project policy at {}", config_path.display())
        })?;
        let policy: ProjectsPolicy = toml::from_str(&content).with_context(|| {
            format!(
                "Failed to parse project policy at {}",
                config_path.display()
            )
        })?;

        let mut entries = Vec::new();
        for (id, project) in policy.projects {
            if project.state.as_deref() != Some("sidecar") {
                continue;
            }
            let Some(sidecar_key) = project.sidecar_key else {
                continue;
            };
            let Some(sidecar_root) = project.sidecar_root.map(PathBuf::from) else {
                continue;
            };
            let state_root = sidecar_root.join("projects").join(&sidecar_key);
            if state_root.exists() {
                continue;
            }
            entries.push(StaleSidecarPolicyEntry {
                id,
                sidecar_key,
                sidecar_root,
                reason: format!("{} does not exist", state_root.display()),
                state_root,
            });
        }
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(entries)
    }

    fn home_dir(&self) -> ExoResult<PathBuf> {
        self.home_dir
            .clone()
            .or_else(platform_home_dir)
            .ok_or_else(|| anyhow!("Shadow project state requires a home directory to be set"))
    }
}

pub(crate) fn platform_home_dir() -> Option<PathBuf> {
    platform_home_dir_from(|name| std::env::var_os(name))
}

#[cfg(windows)]
fn platform_home_dir_from(mut env: impl FnMut(&str) -> Option<OsString>) -> Option<PathBuf> {
    env_path_from(&mut env, "USERPROFILE")
        .or_else(|| {
            let drive = env("HOMEDRIVE")?;
            let path = env("HOMEPATH")?;
            Some(PathBuf::from(drive).join(path))
        })
        .or_else(|| env_path_from(&mut env, "HOME"))
}

#[cfg(not(windows))]
fn platform_home_dir_from(mut env: impl FnMut(&str) -> Option<OsString>) -> Option<PathBuf> {
    env_path_from(&mut env, "HOME")
}

fn config_home_from(
    explicit_config_home: Option<PathBuf>,
    explicit_home_dir: Option<PathBuf>,
    mut env: impl FnMut(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    explicit_config_home
        .or_else(|| {
            #[cfg(windows)]
            {
                env_path_from(&mut env, "APPDATA")
            }
            #[cfg(not(windows))]
            {
                None
            }
        })
        .or_else(|| env_path_from(&mut env, "XDG_CONFIG_HOME"))
        .or_else(|| {
            explicit_home_dir
                .or_else(|| platform_home_dir_from(&mut env))
                .map(|home| home.join(".config"))
        })
}

fn env_path_from(env: &mut impl FnMut(&str) -> Option<OsString>, name: &str) -> Option<PathBuf> {
    env(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

impl ProjectCatalogEntry {
    fn from_resolved_project(project: &Project, source: &'static str, current: bool) -> Self {
        let db_path = project.db_path();
        let workspace_available = project.workspace_root.is_some();
        let state_readable =
            workspace_available || state_root_has_project_state(&project.state_root);
        Self {
            id: project.id.as_str().to_string(),
            display_name: project_display_name(
                project.workspace_root.as_deref(),
                project.sidecar_key.as_deref(),
                project.id.as_str(),
            ),
            source,
            state: project.policy.as_str(),
            workspace_root: project.workspace_root.clone(),
            git_common_dir: Some(project.git_common_dir.clone()),
            state_root: Some(project.state_root.clone()),
            db_path: Some(db_path),
            runtime_dir: Some(project.runtime_dir()),
            sidecar_key: project.sidecar_key.clone(),
            sidecar_root: project.sidecar_root.clone(),
            state_readable,
            workspace_available,
            commands_available: workspace_available,
            write_available: workspace_available,
            selectable: state_readable,
            current,
            diagnostics: Vec::new(),
        }
    }

    fn from_policy(
        id: &str,
        policy: &ProjectPolicy,
        current_project_id: Option<&str>,
        shadow_projects_root: Option<&Path>,
    ) -> Self {
        let sidecar_root = policy.sidecar_root.as_ref().map(PathBuf::from);
        let sidecar_key = policy.sidecar_key.clone();
        let state = policy.catalog_state();
        let state_root = match state {
            "sidecar" => sidecar_root
                .as_ref()
                .zip(sidecar_key.as_ref())
                .map(|(root, key)| root.join("projects").join(key)),
            "shadow" => shadow_projects_root.map(|root| root.join(id)),
            _ => policy.state_root.as_ref().map(PathBuf::from),
        };
        let mut diagnostics = Vec::new();
        if state == "sidecar" && (sidecar_root.is_none() || sidecar_key.is_none()) {
            diagnostics.push(ProjectCatalogDiagnostic {
                source: "local-policy".to_string(),
                severity: "warning",
                message: "sidecar policy is missing sidecar_key or sidecar_root".to_string(),
            });
        }
        if state == "sidecar"
            && let Some(state_root) = state_root.as_ref()
            && !state_root.exists()
        {
            diagnostics.push(ProjectCatalogDiagnostic {
                source: "local-policy".to_string(),
                severity: "warning",
                message: format!(
                    "stale sidecar policy for project {id}: {} does not exist",
                    state_root.display()
                ),
            });
        }
        if state == "shadow" && state_root.is_none() {
            diagnostics.push(ProjectCatalogDiagnostic {
                source: "local-policy".to_string(),
                severity: "warning",
                message: "shadow policy requires a home directory to compute project state root"
                    .to_string(),
            });
        }
        let db_path = state_root.as_ref().map(|root| project_db_path(root));
        let state_readable = state_root
            .as_ref()
            .is_some_and(|root| state_root_has_project_state(root));

        Self {
            id: id.to_string(),
            display_name: project_display_name(None, sidecar_key.as_deref(), id),
            source: "local-policy",
            state,
            workspace_root: None,
            git_common_dir: None,
            db_path,
            runtime_dir: state_root.as_ref().map(|root| root.join("runtime")),
            state_root,
            sidecar_key,
            sidecar_root,
            state_readable,
            workspace_available: false,
            commands_available: false,
            write_available: false,
            selectable: state_readable,
            current: current_project_id == Some(id),
            diagnostics,
        }
    }

    fn is_stale_local_policy_sidecar(&self) -> bool {
        self.source == "local-policy"
            && self.state == "sidecar"
            && self
                .state_root
                .as_ref()
                .is_some_and(|state_root| !state_root.exists())
    }

    fn from_sidecar_project_dir(
        sidecar_root: &Path,
        project_dir: &Path,
        current_project_id: Option<&str>,
    ) -> Self {
        let sidecar_key = project_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_string();
        let manifest_path = project_dir.join("sidecar.toml");
        let mut diagnostics = Vec::new();
        let manifest = match std::fs::read_to_string(&manifest_path) {
            Ok(contents) => match toml::from_str::<SidecarManifest>(&contents) {
                Ok(manifest) => Some(manifest),
                Err(error) => {
                    diagnostics.push(ProjectCatalogDiagnostic {
                        source: "sidecar-root".to_string(),
                        severity: "warning",
                        message: format!("Failed to parse {}: {error}", manifest_path.display()),
                    });
                    None
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                diagnostics.push(ProjectCatalogDiagnostic {
                    source: "sidecar-root".to_string(),
                    severity: "warning",
                    message: format!("Missing sidecar manifest {}", manifest_path.display()),
                });
                None
            }
            Err(error) => {
                diagnostics.push(ProjectCatalogDiagnostic {
                    source: "sidecar-root".to_string(),
                    severity: "warning",
                    message: format!("Failed to read {}: {error}", manifest_path.display()),
                });
                None
            }
        };
        let manifest_sidecar = manifest.and_then(|manifest| manifest.sidecar);
        let id = manifest_sidecar
            .as_ref()
            .and_then(|sidecar| sidecar.project_id.clone())
            .unwrap_or_else(|| sidecar_key.clone());
        let key = manifest_sidecar
            .and_then(|sidecar| sidecar.key)
            .unwrap_or_else(|| sidecar_key.clone());
        let state_root = project_dir.to_path_buf();
        let db_path = project_db_path(&state_root);
        let state_readable = state_root_has_project_state(&state_root);

        Self {
            id: id.clone(),
            display_name: project_display_name(None, Some(&key), &id),
            source: "sidecar-root",
            state: "sidecar",
            workspace_root: None,
            git_common_dir: None,
            db_path: Some(db_path),
            runtime_dir: Some(state_root.join("runtime")),
            state_root: Some(state_root),
            sidecar_key: Some(key),
            sidecar_root: Some(sidecar_root.to_path_buf()),
            state_readable,
            workspace_available: false,
            commands_available: false,
            write_available: false,
            selectable: state_readable,
            current: current_project_id == Some(id.as_str()),
            diagnostics,
        }
    }
}

fn upsert_catalog_entry(
    entries: &mut BTreeMap<String, ProjectCatalogEntry>,
    entry: ProjectCatalogEntry,
) {
    match entries.get_mut(&entry.id) {
        None => {
            entries.insert(entry.id.clone(), entry);
        }
        Some(existing) if existing.source == "current" => {
            if existing.sidecar_key.is_none() {
                existing.sidecar_key = entry.sidecar_key;
            }
            if existing.sidecar_root.is_none() {
                existing.sidecar_root = entry.sidecar_root;
            }
            if existing.state_root.is_none() {
                existing.state_root = entry.state_root;
            }
            if existing.db_path.is_none() {
                existing.db_path = entry.db_path;
            }
            if existing.runtime_dir.is_none() {
                existing.runtime_dir = entry.runtime_dir;
            }
            existing.state_readable = existing.state_readable || entry.state_readable;
            existing.workspace_available =
                existing.workspace_available || entry.workspace_available;
            existing.commands_available = existing.commands_available || entry.commands_available;
            existing.write_available = existing.write_available || entry.write_available;
            existing.diagnostics.extend(entry.diagnostics);
            existing.current = true;
        }
        Some(existing) if existing.source != "current" => {
            let mut replacement = entry;
            replacement.current = replacement.current || existing.current;
            if replacement.workspace_root.is_none() {
                replacement.workspace_root = existing.workspace_root.clone();
            }
            if replacement.git_common_dir.is_none() {
                replacement.git_common_dir = existing.git_common_dir.clone();
            }
            if replacement.state_root.is_none() {
                replacement.state_root = existing.state_root.clone();
            }
            if replacement.db_path.is_none() {
                replacement.db_path = existing.db_path.clone();
            }
            if replacement.runtime_dir.is_none() {
                replacement.runtime_dir = existing.runtime_dir.clone();
            }
            if replacement.sidecar_key.is_none() {
                replacement.sidecar_key = existing.sidecar_key.clone();
            }
            if replacement.sidecar_root.is_none() {
                replacement.sidecar_root = existing.sidecar_root.clone();
            }
            replacement.state_readable = replacement.state_readable || existing.state_readable;
            replacement.workspace_available =
                replacement.workspace_available || existing.workspace_available;
            replacement.commands_available =
                replacement.commands_available || existing.commands_available;
            replacement.write_available = replacement.write_available || existing.write_available;
            replacement.selectable = replacement.selectable || existing.selectable;
            replacement.diagnostics.extend(existing.diagnostics.clone());
            *existing = replacement;
        }
        Some(existing) => {
            existing.diagnostics.extend(entry.diagnostics);
            existing.state_readable = existing.state_readable || entry.state_readable;
            existing.workspace_available =
                existing.workspace_available || entry.workspace_available;
            existing.commands_available = existing.commands_available || entry.commands_available;
            existing.write_available = existing.write_available || entry.write_available;
            existing.selectable = existing.selectable || entry.selectable;
        }
    }
}

fn project_display_name(
    workspace_root: Option<&Path>,
    sidecar_key: Option<&str>,
    fallback_id: &str,
) -> String {
    workspace_root
        .and_then(|root| root.file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .or(sidecar_key)
        .unwrap_or(fallback_id)
        .to_string()
}

fn state_root_has_project_state(state_root: &Path) -> bool {
    project_db_path(state_root).exists()
        || state_root.join("agent-context").join("epochs.sql").exists()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SidecarBinding {
    key: String,
    root: PathBuf,
    auto_commit: bool,
    auto_push: SidecarAutoPushPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarLink {
    pub project: Project,
    pub sidecar_key: String,
    pub sidecar_root: PathBuf,
    pub config_path: PathBuf,
    pub manifest_path: PathBuf,
    pub projection_dir: PathBuf,
    pub db_path: PathBuf,
    pub db_created: bool,
    pub git_initialized: bool,
    pub seeded_from_repo: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SidecarLinkOptions {
    pub key: Option<String>,
    pub root: Option<PathBuf>,
    pub auto_push: Option<SidecarAutoPushPolicy>,
    pub init_git: bool,
    pub seed_from_repo: bool,
}

pub fn init_sidecar(cwd: &Path, options: SidecarLinkOptions) -> ExoResult<SidecarLink> {
    init_sidecar_with_resolver(cwd, options, &ProjectResolver::default())
}

pub(crate) fn init_sidecar_with_resolver(
    cwd: &Path,
    options: SidecarLinkOptions,
    resolver: &ProjectResolver,
) -> ExoResult<SidecarLink> {
    let key = options
        .key
        .filter(|key| !key.trim().is_empty())
        .unwrap_or_else(|| default_sidecar_key(cwd));
    let root = match options.root {
        Some(root) => root,
        None => resolver.default_sidecar_root()?,
    };
    let mut link =
        link_sidecar_with_options_and_resolver(cwd, &key, &root, options.auto_push, resolver)?;
    if options.seed_from_repo {
        link.seeded_from_repo = seed_sidecar_projection_from_repo(cwd, &link.projection_dir)?;
    }
    if link.db_created {
        import_sidecar_projection_to_db(&link.projection_dir, &link.db_path)?;
    }
    if options.init_git {
        link.git_initialized = ensure_sidecar_root_git_repo(&root)?;
    }
    Ok(link)
}

pub fn resolve_sidecar_identity(
    cwd: &Path,
    key: Option<String>,
    root: Option<PathBuf>,
    auto_push: Option<SidecarAutoPushPolicy>,
) -> ExoResult<(ProjectId, String, PathBuf, Option<SidecarAutoPushPolicy>)> {
    resolve_sidecar_identity_with_resolver(cwd, key, root, auto_push, &ProjectResolver::default())
}

pub(crate) fn resolve_sidecar_identity_with_resolver(
    cwd: &Path,
    key: Option<String>,
    root: Option<PathBuf>,
    auto_push: Option<SidecarAutoPushPolicy>,
    resolver: &ProjectResolver,
) -> ExoResult<(ProjectId, String, PathBuf, Option<SidecarAutoPushPolicy>)> {
    let key = key
        .filter(|key| !key.trim().is_empty())
        .unwrap_or_else(|| default_sidecar_key(cwd));
    let root = match root {
        Some(root) => root,
        None => resolver.default_sidecar_root()?,
    };
    let project_id = resolver.resolve(cwd)?.id;
    Ok((project_id, key, root, auto_push))
}

pub fn link_sidecar(cwd: &Path, key: &str, root: &Path) -> ExoResult<SidecarLink> {
    link_sidecar_with_options(cwd, key, root, None)
}

fn link_sidecar_with_options(
    cwd: &Path,
    key: &str,
    root: &Path,
    auto_push: Option<SidecarAutoPushPolicy>,
) -> ExoResult<SidecarLink> {
    link_sidecar_with_options_and_resolver(cwd, key, root, auto_push, &ProjectResolver::default())
}

pub(crate) fn link_sidecar_with_options_and_resolver(
    cwd: &Path,
    key: &str,
    root: &Path,
    auto_push: Option<SidecarAutoPushPolicy>,
    resolver: &ProjectResolver,
) -> ExoResult<SidecarLink> {
    if key.trim().is_empty() {
        anyhow::bail!("sidecar key must not be empty");
    }

    let project_before_policy = resolver.resolve(cwd)?;
    let config_path = resolver.local_projects_config_path()?;
    write_sidecar_policy(
        &config_path,
        &project_before_policy.id,
        key,
        root,
        auto_push,
    )?;

    let project = resolver.resolve(cwd)?;
    let manifest_path = project
        .sidecar_manifest_path()
        .ok_or_else(|| anyhow!("sidecar policy did not resolve a manifest path"))?;
    let projection_dir = project
        .sidecar_projection_dir()
        .ok_or_else(|| anyhow!("sidecar policy did not resolve a projection directory"))?;
    let db_path = project.db_path();

    std::fs::create_dir_all(&projection_dir).with_context(|| {
        format!(
            "Failed to create sidecar projection directory {}",
            projection_dir.display()
        )
    })?;
    if let Some(parent) = manifest_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create sidecar directory {}", parent.display()))?;
    }
    if !manifest_path.exists() {
        let manifest = format!(
            "[sidecar]\nkey = {key:?}\nproject_id = {:?}\n",
            project.id.as_str()
        );
        std::fs::write(&manifest_path, manifest).with_context(|| {
            format!(
                "Failed to write sidecar manifest {}",
                manifest_path.display()
            )
        })?;
    }
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create DB directory {}", parent.display()))?;
    }
    let db_created = !db_path.exists();
    let _db = exosuit_storage::open_database(&db_path)
        .map_err(|error| anyhow!("Failed to create sidecar database: {error}"))?;

    Ok(SidecarLink {
        project,
        sidecar_key: key.to_string(),
        sidecar_root: root.to_path_buf(),
        config_path,
        manifest_path,
        projection_dir,
        db_path,
        db_created,
        git_initialized: false,
        seeded_from_repo: false,
    })
}

fn default_sidecar_key(cwd: &Path) -> String {
    cwd.file_name()
        .and_then(|name| name.to_str())
        .map(slugify_sidecar_key)
        .filter(|key| !key.is_empty())
        .unwrap_or_else(|| "project".to_string())
}

fn slugify_sidecar_key(name: &str) -> String {
    let mut key = String::new();
    let mut previous_dash = false;
    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            key.push(ch);
            previous_dash = false;
        } else if !previous_dash && !key.is_empty() {
            key.push('-');
            previous_dash = true;
        }
    }
    if previous_dash {
        key.pop();
    }
    key
}

fn ensure_sidecar_root_git_repo(root: &Path) -> ExoResult<bool> {
    std::fs::create_dir_all(root)
        .with_context(|| format!("Failed to create sidecar root {}", root.display()))?;
    let output = run_git(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    if output.status.success() {
        let expected_git_dir = root.join(".git").canonicalize().ok();
        let actual_git_dir = git_stdout_path(&output).and_then(|path| path.canonicalize().ok());
        if actual_git_dir == expected_git_dir {
            ensure_sidecar_git_identity(root)?;
            ensure_sidecar_runtime_gitignore(root)?;
            return Ok(false);
        }
    }

    let output = Command::new("git")
        .arg("init")
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("Failed to run git init in {}", root.display()))?;
    if !output.status.success() {
        return Err(git_command_error(root, "git init", &output));
    }
    ensure_sidecar_git_identity(root)?;
    ensure_sidecar_runtime_gitignore(root)?;
    Ok(true)
}

pub(crate) fn ensure_sidecar_git_identity(root: &Path) -> ExoResult<()> {
    ensure_sidecar_git_config_value(root, "user.name", SIDECAR_GIT_USER_NAME)?;
    ensure_sidecar_git_config_value(root, "user.email", SIDECAR_GIT_USER_EMAIL)
}

fn ensure_sidecar_git_config_value(root: &Path, key: &str, value: &str) -> ExoResult<()> {
    let output = run_git(root, &["config", "--local", "--get", key])?;
    if output.status.success() && !String::from_utf8_lossy(&output.stdout).trim().is_empty() {
        return Ok(());
    }

    let output = run_git(root, &["config", "--local", key, value])?;
    if !output.status.success() {
        return Err(git_command_error(
            root,
            &format!("git config --local {key}"),
            &output,
        ));
    }

    Ok(())
}

fn ensure_sidecar_runtime_gitignore(root: &Path) -> ExoResult<()> {
    let path = root.join(".gitignore");
    let mut contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to read sidecar gitignore {}", path.display()));
        }
    };
    let existing: std::collections::HashSet<String> = contents
        .lines()
        .map(|line| line.trim().to_string())
        .collect();
    let mut changed = false;
    for entry in SIDECAR_RUNTIME_GITIGNORE_ENTRIES {
        if !existing.contains(*entry) {
            if !contents.is_empty() && !contents.ends_with('\n') {
                contents.push('\n');
            }
            contents.push_str(entry);
            contents.push('\n');
            changed = true;
        }
    }
    if changed {
        std::fs::write(&path, contents)
            .with_context(|| format!("Failed to write sidecar gitignore {}", path.display()))?;
    }
    Ok(())
}

fn seed_sidecar_projection_from_repo(cwd: &Path, projection_dir: &Path) -> ExoResult<bool> {
    let repo_projection = cwd.join("docs/agent-context");
    if !repo_projection.exists() {
        return Ok(false);
    }

    std::fs::create_dir_all(projection_dir).with_context(|| {
        format!(
            "Failed to create sidecar projection directory {}",
            projection_dir.display()
        )
    })?;

    let mut copied_any = false;
    for entry in std::fs::read_dir(&repo_projection).with_context(|| {
        format!(
            "Failed to read repo projection directory {}",
            repo_projection.display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!(
                "Failed to read repo projection entry in {}",
                repo_projection.display()
            )
        })?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("sql") {
            continue;
        }
        let dest = projection_dir.join(entry.file_name());
        if dest.exists() {
            continue;
        }
        std::fs::copy(&path, &dest).with_context(|| {
            format!(
                "Failed to seed sidecar projection {} from {}",
                dest.display(),
                path.display()
            )
        })?;
        copied_any = true;
    }

    Ok(copied_any)
}

fn import_sidecar_projection_to_db(projection_dir: &Path, db_path: &Path) -> ExoResult<()> {
    let has_any_dump = exosuit_storage::TABLE_ORDER.iter().any(|(file_stem, _)| {
        projection_dir
            .join(format!("{file_stem}.sql"))
            .metadata()
            .is_ok_and(|metadata| metadata.len() > 0)
    });
    if !has_any_dump {
        return Ok(());
    }

    let db = exosuit_storage::open_database(db_path)
        .map_err(|error| anyhow!("Failed to open sidecar database: {error}"))?;
    let mut dumps = Vec::new();
    for (file_stem, table_name) in exosuit_storage::TABLE_ORDER {
        let path = projection_dir.join(format!("{file_stem}.sql"));
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to read {}", path.display()));
            }
        };
        dumps.push((table_name.to_string(), content));
    }
    exosuit_storage::import_tables(db.connection(), &dumps)
        .map_err(|error| anyhow!("Failed to import sidecar projection: {error}"))
}

pub fn unlink_sidecar(cwd: &Path) -> ExoResult<Option<(ProjectId, PathBuf)>> {
    unlink_sidecar_with_resolver(cwd, &ProjectResolver::default())
}

pub(crate) fn unlink_sidecar_with_resolver(
    cwd: &Path,
    resolver: &ProjectResolver,
) -> ExoResult<Option<(ProjectId, PathBuf)>> {
    let project = resolver.resolve(cwd)?;
    if project.policy != StatePolicy::Sidecar {
        return Ok(None);
    }
    let config_path = resolver.local_projects_config_path()?;
    let removed = remove_sidecar_policy(&config_path, &project.id)?;
    Ok(removed.then_some((project.id, config_path)))
}

fn write_sidecar_policy(
    config_path: &Path,
    project_id: &ProjectId,
    key: &str,
    root: &Path,
    auto_push: Option<SidecarAutoPushPolicy>,
) -> ExoResult<()> {
    let mut doc = read_projects_doc(config_path)?;
    ensure_table(&mut doc, "projects")?;
    doc["projects"][project_id.as_str()]["state"] = value("sidecar");
    doc["projects"][project_id.as_str()]["sidecar_key"] = value(key);
    doc["projects"][project_id.as_str()]["sidecar_root"] = value(root.to_string_lossy().as_ref());
    if let Some(auto_push) = auto_push {
        doc["projects"][project_id.as_str()]["auto_push"] = value(auto_push.as_str());
    }
    write_projects_doc(config_path, &doc)
}

fn remove_sidecar_policy(config_path: &Path, project_id: &ProjectId) -> ExoResult<bool> {
    if !config_path.exists() {
        return Ok(false);
    }

    let mut doc = read_projects_doc(config_path)?;
    let Some(projects) = doc["projects"].as_table_mut() else {
        return Ok(false);
    };
    let removed = projects.remove(project_id.as_str()).is_some();
    if removed {
        write_projects_doc(config_path, &doc)?;
    }
    Ok(removed)
}

fn read_projects_doc(config_path: &Path) -> ExoResult<DocumentMut> {
    if !config_path.exists() {
        return Ok(DocumentMut::new());
    }

    let content = std::fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read {}", config_path.display()))?;
    content
        .parse::<DocumentMut>()
        .with_context(|| format!("Failed to parse {}", config_path.display()))
}

fn write_projects_doc(config_path: &Path, doc: &DocumentMut) -> ExoResult<()> {
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory {}", parent.display()))?;
    }
    std::fs::write(config_path, doc.to_string())
        .with_context(|| format!("Failed to write {}", config_path.display()))
}

fn ensure_table(doc: &mut DocumentMut, key: &str) -> ExoResult<()> {
    if doc.get(key).is_none() {
        doc[key] = Item::Table(toml_edit::Table::new());
    }
    if doc[key].as_table().is_none() {
        anyhow::bail!("{key} must be a TOML table");
    }
    Ok(())
}

#[derive(Debug, Deserialize, Default)]
struct ProjectsPolicy {
    #[serde(default)]
    projects: HashMap<String, ProjectPolicy>,
}

#[derive(Debug, Deserialize, Default)]
struct ProjectPolicy {
    #[serde(default)]
    shadow: bool,
    state: Option<String>,
    state_root: Option<String>,
    sidecar_key: Option<String>,
    sidecar_root: Option<String>,
    auto_commit: Option<bool>,
    auto_push: Option<String>,
}

impl ProjectPolicy {
    fn uses_shadow_state(&self) -> bool {
        self.shadow
            || self.state.as_deref() == Some("shadow")
            || self.state_root.as_deref() == Some("shadow")
    }

    fn catalog_state(&self) -> &'static str {
        if self.state.as_deref() == Some("sidecar") {
            "sidecar"
        } else if self.uses_shadow_state() {
            "shadow"
        } else {
            "repo"
        }
    }
}

#[derive(Debug, Deserialize)]
struct SidecarManifest {
    sidecar: Option<SidecarManifestSection>,
}

#[derive(Debug, Deserialize)]
struct SidecarManifestSection {
    key: Option<String>,
    project_id: Option<String>,
}

fn run_git(cwd: &Path, args: &[&str]) -> ExoResult<Output> {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("Failed to run git in {}", cwd.display()))
}

fn git_stdout_path(output: &Output) -> Option<PathBuf> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let path = stdout.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn git_required(output: &Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr.contains("not a git repository") || stderr.contains("not in a git directory")
}

fn git_command_error(cwd: &Path, command: &str, output: &Output) -> anyhow::Error {
    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow!("{command} failed in {}: {}", cwd.display(), stderr.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn run_git_ok(cwd: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "git {} failed in {}: {}",
            args.join(" "),
            cwd.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_primary_repo() -> (TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        run_git_ok(&repo, &["init"]);
        std::fs::write(repo.join("README.md"), "# test\n").unwrap();
        run_git_ok(&repo, &["add", "README.md"]);
        run_git_ok(
            &repo,
            &[
                "-c",
                "user.name=Exo Test",
                "-c",
                "user.email=exo@example.invalid",
                "commit",
                "-m",
                "init",
            ],
        );
        (temp, repo)
    }

    fn git_common_dir(cwd: &Path) -> PathBuf {
        let output = Command::new("git")
            .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(output.status.success());
        PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
            .canonicalize()
            .unwrap()
    }

    fn resolver_with_test_home(temp: &TempDir) -> ProjectResolver {
        ProjectResolver::default()
            .with_home_dir(temp.path().join("home"))
            .with_config_home(temp.path().join("xdg-config"))
    }

    #[cfg(windows)]
    #[test]
    fn platform_home_dir_prefers_native_windows_profile_over_msys_home() {
        let env = HashMap::from([
            ("HOME", OsString::from("/c/Users/alice")),
            ("USERPROFILE", OsString::from(r"C:\Users\alice")),
        ]);

        let home = platform_home_dir_from(|name| env.get(name).cloned());

        assert_eq!(home, Some(PathBuf::from(r"C:\Users\alice")));
    }

    #[cfg(windows)]
    #[test]
    fn config_home_prefers_native_windows_appdata_over_msys_xdg() {
        let env = HashMap::from([
            ("XDG_CONFIG_HOME", OsString::from("/c/Users/alice/.config")),
            ("APPDATA", OsString::from(r"C:\Users\alice\AppData\Roaming")),
        ]);

        let config_home = config_home_from(None, None, |name| env.get(name).cloned());

        assert_eq!(
            config_home,
            Some(PathBuf::from(r"C:\Users\alice\AppData\Roaming"))
        );
    }

    #[test]
    fn primary_worktree_uses_gitdir_adjacent_state_root_by_default() {
        let (temp, repo) = init_primary_repo();
        let project = resolver_with_test_home(&temp).resolve(&repo).unwrap();
        let common_dir = git_common_dir(&repo);
        let canonical_repo = repo.canonicalize().unwrap();

        assert_eq!(project.id, ProjectId::from_git_common_dir(&common_dir));
        assert_eq!(project.git_common_dir, common_dir);
        assert_eq!(project.workspace_root, Some(canonical_repo.clone()));
        assert_eq!(project.policy, StatePolicy::Repo);
        assert_eq!(project.state_root, canonical_repo.join(".exo"));
        assert_eq!(
            project.db_path(),
            canonical_repo.join(".exo").join("cache").join("exo.db")
        );
        assert_eq!(project.runtime_dir(), canonical_repo.join(".exo/runtime"));
        assert_eq!(
            project.socket_path(),
            canonical_repo.join(".exo/runtime/daemon.sock")
        );
        assert_eq!(
            project.pid_path(),
            canonical_repo.join(".exo/runtime/daemon.pid")
        );
    }

    #[test]
    fn long_state_roots_use_short_project_socket_path() {
        let (temp, repo) = init_primary_repo();
        let mut project = resolver_with_test_home(&temp).resolve(&repo).unwrap();
        let long_component = "very-long-sidecar-root-component".repeat(5);
        project.state_root = temp
            .path()
            .join(long_component)
            .join("projects")
            .join("exo2");

        assert!(
            project.socket_path().starts_with("/tmp/exo-daemon-sockets"),
            "long project socket paths should use a stable short temp socket: {}",
            project.socket_path().display()
        );
        assert!(project.socket_path().to_string_lossy().len() < MAX_PORTABLE_UNIX_SOCKET_PATH_LEN);
    }

    #[test]
    fn linked_worktree_shares_project_id_and_default_state_root() {
        let (temp, repo) = init_primary_repo();
        let linked = temp.path().join("linked");
        run_git_ok(
            &repo,
            &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
        );

        let resolver = resolver_with_test_home(&temp);
        let primary_project = resolver.resolve(&repo).unwrap();
        let linked_project = resolver.resolve(&linked).unwrap();

        assert_eq!(linked_project.id, primary_project.id);
        assert_eq!(
            linked_project.git_common_dir,
            primary_project.git_common_dir
        );
        assert_eq!(linked_project.state_root, primary_project.state_root);
        assert_eq!(
            linked_project.workspace_root,
            Some(linked.canonicalize().unwrap())
        );
    }

    #[test]
    fn non_git_directory_returns_friendly_require_git_error() {
        let temp = tempfile::tempdir().unwrap();
        let err = resolver_with_test_home(&temp)
            .resolve(temp.path())
            .unwrap_err();
        let message = err.to_string();

        assert!(message.contains("requires a git repository"));
        assert!(message.contains("git init"));
        assert!(!message.contains("fatal:"));
    }

    #[test]
    fn bare_repo_has_project_identity_and_no_workspace_root() {
        let temp = tempfile::tempdir().unwrap();
        let bare = temp.path().join("repo.git");
        std::fs::create_dir(&bare).unwrap();
        run_git_ok(&bare, &["init", "--bare"]);

        let project = resolver_with_test_home(&temp).resolve(&bare).unwrap();
        let common_dir = bare.canonicalize().unwrap();

        assert_eq!(project.id, ProjectId::from_git_common_dir(&common_dir));
        assert_eq!(project.git_common_dir, common_dir);
        assert_eq!(project.workspace_root, None);
        assert_eq!(project.policy, StatePolicy::Repo);
        assert_eq!(
            project.state_root,
            temp.path().canonicalize().unwrap().join(".exo")
        );
    }

    #[test]
    fn shadow_policy_maps_primary_and_linked_worktree_to_project_home() {
        let (temp, repo) = init_primary_repo();
        let linked = temp.path().join("linked");
        run_git_ok(
            &repo,
            &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
        );

        let resolver = resolver_with_test_home(&temp);
        let id = ProjectId::from_git_common_dir(&git_common_dir(&repo));
        let config_dir = temp.path().join("xdg-config/exo");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("projects.toml"),
            format!("[projects.{}]\nshadow = true\n", id.as_str()),
        )
        .unwrap();

        let primary_project = resolver.resolve(&repo).unwrap();
        let linked_project = resolver.resolve(&linked).unwrap();
        let expected = temp.path().join("home/.exo/projects").join(id.as_str());

        assert_eq!(primary_project.state_root, expected);
        assert_eq!(primary_project.policy, StatePolicy::Shadow);
        assert_eq!(linked_project.id, primary_project.id);
        assert_eq!(linked_project.policy, StatePolicy::Shadow);
        assert_eq!(linked_project.state_root, primary_project.state_root);
    }

    #[test]
    fn repo_policy_state_value_uses_repo_state_root() {
        let (temp, repo) = init_primary_repo();
        let resolver = resolver_with_test_home(&temp);
        let id = ProjectId::from_git_common_dir(&git_common_dir(&repo));
        let config_dir = temp.path().join("xdg-config/exo");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("projects.toml"),
            format!("[projects.{}]\nstate = \"repo\"\n", id.as_str()),
        )
        .unwrap();

        let project = resolver.resolve(&repo).unwrap();
        let canonical_repo = repo.canonicalize().unwrap();

        assert_eq!(project.policy, StatePolicy::Repo);
        assert_eq!(project.state_root, canonical_repo.join(".exo"));
    }

    #[test]
    fn legacy_default_policy_state_value_uses_repo_state_root() {
        let (temp, repo) = init_primary_repo();
        let resolver = resolver_with_test_home(&temp);
        let id = ProjectId::from_git_common_dir(&git_common_dir(&repo));
        let config_dir = temp.path().join("xdg-config/exo");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("projects.toml"),
            format!("[projects.{}]\nstate = \"default\"\n", id.as_str()),
        )
        .unwrap();

        let project = resolver.resolve(&repo).unwrap();
        let canonical_repo = repo.canonicalize().unwrap();

        assert_eq!(project.policy, StatePolicy::Repo);
        assert_eq!(project.state_root, canonical_repo.join(".exo"));
    }

    #[test]
    fn sidecar_policy_maps_to_portable_project_state_and_projection() {
        let (temp, repo) = init_primary_repo();
        let resolver = resolver_with_test_home(&temp);
        let id = ProjectId::from_git_common_dir(&git_common_dir(&repo));
        let config_dir = temp.path().join("xdg-config/exo");
        let sidecar_root = temp.path().join("portable-sidecars");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("projects.toml"),
            format!(
                "[projects.{}]\nstate = \"sidecar\"\nsidecar_key = \"client-api\"\nsidecar_root = {:?}\n",
                id.as_str(),
                sidecar_root.to_string_lossy()
            ),
        )
        .unwrap();

        let project = resolver.resolve(&repo).unwrap();

        assert_eq!(project.policy, StatePolicy::Sidecar);
        assert_eq!(project.sidecar_key.as_deref(), Some("client-api"));
        assert_eq!(
            project.sidecar_root.as_deref(),
            Some(sidecar_root.as_path())
        );
        assert_eq!(
            project.state_root,
            sidecar_root.join("projects").join("client-api")
        );
        assert_eq!(
            project.db_path(),
            sidecar_root
                .join("projects")
                .join("client-api")
                .join("cache")
                .join("exo.db")
        );
        assert_eq!(
            project.runtime_dir(),
            sidecar_root
                .join("projects")
                .join("client-api")
                .join("runtime")
        );
        assert_eq!(
            project.sidecar_manifest_path(),
            Some(
                sidecar_root
                    .join("projects")
                    .join("client-api")
                    .join("sidecar.toml")
            )
        );
        assert_eq!(
            project.sidecar_projection_dir(),
            Some(
                sidecar_root
                    .join("projects")
                    .join("client-api")
                    .join("agent-context")
            )
        );
    }
}
