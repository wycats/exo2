//! Sidecar namespace commands.
//!
//! - `sidecar init`: Create or reuse portable sidecar state for this git repo
//! - `sidecar bootstrap`: Turn this git repo into a sidecar-backed Exosuit repo
//! - `sidecar discover`: Report discovered sidecar configuration for this git repo
//! - `sidecar link`: Bind this git repo to portable sidecar state
//! - `sidecar status`: Show the current sidecar binding
//! - `sidecar setup`: Create missing GitHub sidecar setup with approval
//! - `sidecar repo <status|commit|remote|push|sync>`: Manage the sidecar git repository
//! - `sidecar unlink`: Remove the local sidecar binding

use super::traits::{Command, CommandBox, CommandContext, CommandOutput, OutputFormat};
use crate::api::protocol::{Effect, ErrorCode};
use crate::command_reference::ExoCommandReference;
use crate::failure::ExoFailure;
use crate::github::fetch::{
    CurrentSidecarRegistryFetcher, SidecarRegistryCheckedAttempt, SidecarRegistryFetchAttempt,
    SidecarRegistryFetchReport, SidecarRegistryFetchRequest, SidecarRegistryFetchStatus,
    SidecarRegistryFetchedRegistry, SidecarRegistryFetcher,
};
use crate::github::registry::{
    SidecarDiscoveryConfidence, SidecarRegistryFailureClassification, SidecarRegistryMatchKind,
    SidecarRegistryProposal, SidecarRegistryResolution, resolve_sidecar_registry,
};
use crate::github::remote::{ParsedGithubRemote, parse_github_remote};
use crate::project::{
    Project, ProjectResolver, SidecarAutoPushPolicy, SidecarLinkOptions, StatePolicy,
    init_sidecar_with_resolver, link_sidecar_with_options_and_resolver,
    resolve_sidecar_identity_with_resolver, unlink_sidecar_with_resolver,
};
use crate::steering::{SuggestedAction, WorkIntent};
use anyhow::{Context, Result as ExoResult};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::PathBuf;
use std::path::{Component, Path};
use std::process::{Command as ProcessCommand, Output};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_REMOTE: &str = "origin";
const SIDECAR_RUNTIME_GITIGNORE_ENTRIES: &[&str] = &["projects/*/cache/", "projects/*/runtime/"];

fn project_resolver_for_context(project: Option<&Project>) -> ProjectResolver {
    match project.and_then(|project| project.projects_config_path.as_deref()) {
        Some(path) => ProjectResolver::default().with_projects_config_path(path),
        None => ProjectResolver::default(),
    }
}

fn resolve_project_for_context(root: &Path, project: Option<&Project>) -> ExoResult<Project> {
    project_resolver_for_context(project).resolve(root)
}

#[derive(Debug, exospec::ExoSpec)]
#[exo(namespace = "sidecar", description = "Portable sidecar state commands")]
pub enum SidecarCommands {
    #[exo(
        effect = "write",
        description = "Bootstrap sidecar-backed Exosuit state for this repo"
    )]
    Bootstrap {
        #[exo(long, optional, description = "Portable sidecar key")]
        key: Option<String>,
        #[exo(long, optional, description = "Portable sidecar root directory")]
        root: Option<String>,
        #[exo(flag, description = "Do not initialize a git repo at the sidecar root")]
        no_git: bool,
        #[exo(flag, description = "Use discovered sidecar configuration")]
        discover: bool,
        #[exo(long, optional, description = "Read registry TOML from a local file")]
        registry_file: Option<String>,
        #[exo(flag, description = "Accept a discovered template/default remote")]
        accept_discovered_remote: bool,
    },

    #[exo(
        effect = "write",
        description = "Create or reuse sidecar state for this repo"
    )]
    Init {
        #[exo(long, optional, description = "Portable sidecar key")]
        key: Option<String>,
        #[exo(long, optional, description = "Portable sidecar root directory")]
        root: Option<String>,
        #[exo(flag, description = "Initialize a git repo at the sidecar root")]
        git: bool,
    },

    #[exo(
        effect = "pure",
        description = "Discover sidecar configuration for this repo"
    )]
    Discover {
        #[exo(long, optional, description = "Read registry TOML from a local file")]
        registry_file: Option<String>,
    },

    #[exo(effect = "write", description = "Bind this repo to sidecar state")]
    Link {
        #[exo(long, description = "Portable sidecar key")]
        key: String,
        #[exo(long, description = "Portable sidecar root directory")]
        root: String,
    },

    #[exo(effect = "pure", description = "Show sidecar binding status")]
    Status {
        #[exo(long, optional, description = "Read registry TOML from a local file")]
        registry_file: Option<String>,
    },

    #[exo(
        effect = "write",
        description = "Create a local checkpoint for sidecar-backed project state"
    )]
    Checkpoint {
        #[exo(long, optional, description = "Sidecar project key to checkpoint")]
        project: Option<String>,
        #[exo(long, short = 'm', optional, description = "Checkpoint commit message")]
        message: Option<String>,
    },

    #[exo(
        effect = "exec",
        description = "Create missing GitHub sidecar repo and profile registry setup"
    )]
    Setup {
        #[exo(
            flag,
            description = "Show planned setup without creating GitHub resources"
        )]
        dry_run: bool,
        #[exo(
            long,
            optional,
            description = "GitHub profile owner for .exosuit/sidecars.toml"
        )]
        profile_owner: Option<String>,
        #[exo(long, optional, description = "GitHub sidecar state repository name")]
        state_repo: Option<String>,
        #[exo(
            long,
            optional,
            description = "Remote URL for sidecar state repository"
        )]
        remote_url: Option<String>,
        #[exo(
            flag,
            description = "Replace an existing origin remote on the sidecar repository"
        )]
        replace_remote: bool,
    },

    #[exo(
        effect = "write",
        description = "Inspect or mutate the sidecar git repo. The status action is read-only at runtime, but this combined operation is classified as write for command-spec metadata."
    )]
    Repo {
        #[exo(
            positional,
            description = "Repo action: status, commit, remote, push, or sync"
        )]
        action: String,
        #[exo(long, short = 'm', optional, description = "Commit message")]
        message: Option<String>,
        #[exo(long, optional, description = "Remote to push to")]
        remote: Option<String>,
        #[exo(long, optional, description = "Branch to push")]
        branch: Option<String>,
        #[exo(long, optional, description = "Remote URL for sidecar repo remote")]
        url: Option<String>,
        #[exo(flag, description = "Replace an existing sidecar repo remote URL")]
        replace: bool,
    },

    #[exo(
        effect = "write",
        description = "Remove this repo's local sidecar binding"
    )]
    Unlink,
}

impl SidecarCommands {
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Bootstrap {
                key,
                root,
                no_git,
                discover,
                registry_file,
                accept_discovered_remote,
            } => CommandBox::pure(SidecarBootstrap::new(
                key,
                root.map(PathBuf::from),
                no_git,
                discover,
                registry_file.map(PathBuf::from),
                accept_discovered_remote,
            )),
            Self::Init { key, root, git } => {
                CommandBox::pure(SidecarInit::new(key, root.map(PathBuf::from), git))
            }
            Self::Discover { registry_file } => {
                CommandBox::pure(SidecarDiscover::new(registry_file.map(PathBuf::from)))
            }
            Self::Link { key, root } => {
                CommandBox::pure(SidecarLink::new(key, PathBuf::from(root)))
            }
            Self::Status { registry_file } => {
                CommandBox::pure(SidecarStatus::new(registry_file.map(PathBuf::from)))
            }
            Self::Checkpoint { project, message } => {
                CommandBox::pure(SidecarCheckpoint::new(project, message))
            }
            Self::Setup {
                dry_run,
                profile_owner,
                state_repo,
                remote_url,
                replace_remote,
            } => CommandBox::pure(SidecarSetup::new(
                dry_run,
                profile_owner,
                state_repo,
                remote_url,
                replace_remote,
            )),
            Self::Repo {
                action,
                message,
                remote,
                branch,
                url,
                replace,
            } => CommandBox::pure(SidecarRepo::new(
                action, message, remote, branch, url, replace,
            )),
            Self::Unlink => CommandBox::pure(SidecarUnlink::new()),
        })
    }
}

#[derive(Debug, Clone)]
pub struct SidecarBootstrap {
    key: Option<String>,
    root: Option<PathBuf>,
    no_git: bool,
    discover: bool,
    registry_file: Option<PathBuf>,
    accept_discovered_remote: bool,
}

impl SidecarBootstrap {
    pub const fn new(
        key: Option<String>,
        root: Option<PathBuf>,
        no_git: bool,
        discover: bool,
        registry_file: Option<PathBuf>,
        accept_discovered_remote: bool,
    ) -> Self {
        Self {
            key,
            root,
            no_git,
            discover,
            registry_file,
            accept_discovered_remote,
        }
    }
}

#[derive(Debug, Serialize)]
struct SidecarBootstrapOutput {
    kind: &'static str,
    ok: bool,
    ready: bool,
    project_id: String,
    sidecar_key: String,
    sidecar_root: PathBuf,
    sidecar_root_source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_sidecar_root: Option<PathBuf>,
    known_sidecar_roots: Vec<SidecarKnownRootOutput>,
    config_path: PathBuf,
    manifest_path: PathBuf,
    projection_dir: PathBuf,
    db_path: PathBuf,
    db_created: bool,
    git_initialized: bool,
    seeded_from_repo: bool,
    repo_clean: bool,
    has_remote: bool,
    remote: Option<String>,
    branch: Option<String>,
    sync_issue: Option<String>,
    discovery: Option<SidecarDiscoveryOutput>,
    next_actions: Vec<SuggestedAction>,
}

#[derive(Debug, Clone, Serialize)]
struct SidecarKnownRootOutput {
    root: PathBuf,
    source: &'static str,
    is_default: bool,
    project_keys: Vec<String>,
}

impl Command for SidecarBootstrap {
    fn namespace(&self) -> &'static str {
        "sidecar"
    }

    fn operation(&self) -> &'static str {
        "bootstrap"
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let resolver = project_resolver_for_context(ctx.project);
        let discovery = if self.discover {
            let discovery = discover_sidecar(ctx.root, self.registry_file.as_deref());
            if !discovery.ok {
                if ctx.format == OutputFormat::Json {
                    return Ok(CommandOutput::data(discovery));
                }
                return Err(anyhow::Error::new(ExoFailure::new(
                    ErrorCode::PreconditionFailed,
                    discovery
                        .failure
                        .as_ref()
                        .map(|failure| failure.message.clone())
                        .unwrap_or_else(|| {
                            "sidecar discovery did not produce a usable proposal".to_string()
                        }),
                    ExoFailure::orienting_steering(discovery.next_actions),
                )));
            }
            let Some(proposal) = discovery.proposal.as_ref() else {
                return Err(anyhow::Error::new(ExoFailure::new(
                    ErrorCode::PreconditionFailed,
                    "sidecar discovery did not produce a proposal".to_string(),
                    ExoFailure::orienting_steering(discovery.next_actions),
                )));
            };
            if proposal.requires_remote_acceptance && !self.accept_discovered_remote {
                return Err(anyhow::Error::new(ExoFailure::new(
                    ErrorCode::PreconditionFailed,
                    "discovered sidecar remote requires --accept-discovered-remote".to_string(),
                    ExoFailure::orienting_steering(discovery.next_actions.clone()),
                )));
            }
            Some(discovery)
        } else {
            None
        };

        if discovery.is_none() && workspace_requires_git_init(ctx.root) {
            let mut details = serde_json::json!({
                "kind": "sidecar.bootstrap",
                "requires_git_repo": true,
                "next_command": "git init",
            });
            if let Ok(default_root) = resolver.default_sidecar_root() {
                details["default_sidecar_root"] =
                    serde_json::Value::String(default_root.to_string_lossy().to_string());
            }
            return Err(anyhow::Error::new(
                ExoFailure::new(
                    ErrorCode::PreconditionFailed,
                    "sidecar bootstrap requires a git repository; run `git init` first".to_string(),
                    ExoFailure::orienting_steering(vec![git_init_suggested_action()]),
                )
                .with_details(details),
            ));
        }
        let discovered_proposal = discovery
            .as_ref()
            .and_then(|discovery| discovery.proposal.as_ref());
        let existing_project = resolver.resolve(ctx.root).ok();
        let existing_sidecar = existing_project
            .as_ref()
            .filter(|project| project.policy == StatePolicy::Sidecar);
        let existing_sidecar_root =
            existing_sidecar.and_then(|project| project.sidecar_root.clone());
        let discovered_root = if self.root.is_none() {
            discovered_proposal
                .and_then(|proposal| proposal.root.as_deref())
                .map(expand_discovered_root)
                .transpose()?
        } else {
            None
        };
        let sidecar_key = self.key.clone().or_else(|| {
            discovered_proposal
                .map(|proposal| proposal.key.clone())
                .or_else(|| existing_sidecar.and_then(|project| project.sidecar_key.clone()))
        });
        let sidecar_root = self.root.clone().or_else(|| {
            discovered_root
                .clone()
                .or_else(|| existing_sidecar_root.clone())
        });
        let auto_push = discovered_proposal
            .and_then(|proposal| proposal.auto_push.as_deref())
            .and_then(parse_auto_push_policy);
        if !self.no_git
            && let Some(discovered_remote) =
                discovered_proposal.and_then(|proposal| proposal.remote.as_deref())
        {
            let (_project_id, _key, resolved_root, _auto_push) =
                resolve_sidecar_identity_with_resolver(
                    ctx.root,
                    sidecar_key.clone(),
                    sidecar_root.clone(),
                    auto_push,
                    &resolver,
                )?;
            adopt_discovered_sidecar_remote(&resolved_root, discovered_remote)?;
        }

        let link = init_sidecar_with_resolver(
            ctx.root,
            SidecarLinkOptions {
                key: sidecar_key,
                root: sidecar_root,
                auto_push,
                init_git: !self.no_git,
                seed_from_repo: true,
            },
            &resolver,
        )?;
        let sidecar_root = link.sidecar_root.clone();
        let repo_status = (!self.no_git)
            .then(|| {
                let repo = ResolvedSidecarRepo {
                    project: link.project.clone(),
                    sidecar_root: sidecar_root.clone(),
                };
                read_sidecar_repo_sync_status(&repo)
            })
            .transpose()?;
        let next_actions = bootstrap_next_actions(repo_status.as_ref(), self.no_git);
        let sync_issue = repo_status
            .as_ref()
            .and_then(|status| status.issue.clone())
            .or_else(|| {
                self.no_git
                    .then(|| "sidecar git initialization skipped".to_string())
            });
        let default_sidecar_root = resolver.default_sidecar_root().ok();
        let sidecar_root_source = sidecar_root_source(
            self.root.as_ref(),
            discovered_root.as_ref(),
            existing_sidecar_root.as_ref(),
            &sidecar_root,
            default_sidecar_root.as_deref(),
        );
        let known_sidecar_roots = known_sidecar_roots(&resolver, default_sidecar_root.as_deref())?;
        let output = SidecarBootstrapOutput {
            kind: "sidecar.bootstrap",
            ok: true,
            ready: sync_issue.is_none(),
            project_id: link.project.id.as_str().to_string(),
            sidecar_key: link.sidecar_key,
            sidecar_root: sidecar_root.clone(),
            sidecar_root_source,
            default_sidecar_root,
            known_sidecar_roots,
            config_path: link.config_path,
            manifest_path: link.manifest_path,
            projection_dir: link.projection_dir,
            db_path: link.db_path,
            db_created: link.db_created,
            git_initialized: link.git_initialized,
            seeded_from_repo: link.seeded_from_repo,
            repo_clean: repo_status.as_ref().is_some_and(|status| status.repo_clean),
            has_remote: repo_status.as_ref().is_some_and(|status| status.has_remote),
            remote: repo_status
                .as_ref()
                .and_then(|status| status.remote.clone()),
            branch: repo_status
                .as_ref()
                .and_then(|status| status.branch.clone()),
            sync_issue,
            discovery,
            next_actions,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let message = format_bootstrap_human_message(&output);
                Ok(CommandOutput::new(output, message))
            }
        }
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn description(&self) -> &'static str {
        "Bootstrap sidecar-backed Exosuit state for this repo"
    }
}

fn adopt_discovered_sidecar_remote(sidecar_root: &Path, discovered_remote: &str) -> ExoResult<()> {
    if !sidecar_root.exists() {
        return clone_discovered_sidecar_remote(sidecar_root, discovered_remote);
    }
    if directory_is_empty(sidecar_root)? {
        return clone_discovered_sidecar_remote(sidecar_root, discovered_remote);
    }
    if !is_independent_git_repo(sidecar_root)? {
        return Err(anyhow::Error::new(ExoFailure::new(
            ErrorCode::PreconditionFailed,
            format!(
                "discovered sidecar root {} already exists and is not an independent git repository; refusing to write sidecar state before adopting discovered remote '{discovered_remote}'",
                sidecar_root.display()
            ),
            ExoFailure::orienting_steering(vec![SuggestedAction {
                label: "Inspect sidecar root".to_string(),
                command: format!(
                    "git -C {} status",
                    shell_quote_arg(&sidecar_root.display().to_string())
                ),
                rationale:
                    "Move, empty, or clone the discovered sidecar root before retrying bootstrap."
                        .to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.9),
            }]),
        )));
    }
    let existing_remotes = remote_names(sidecar_root)?;
    if let Some(existing_remote) = existing_remotes
        .iter()
        .find(|remote| remote.as_str() != DEFAULT_REMOTE)
    {
        return Err(anyhow::Error::new(ExoFailure::new(
            ErrorCode::PreconditionFailed,
            format!(
                "sidecar repo already has remote '{existing_remote}'; refusing to add discovered remote '{DEFAULT_REMOTE}'"
            ),
            ExoFailure::orienting_steering(vec![SuggestedAction {
                label: "Inspect sidecar remote".to_string(),
                command: "exo sidecar repo status".to_string(),
                rationale: "Existing sidecar remotes are preserved until explicitly changed."
                    .to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(0.9),
            }]),
        )));
    }
    let existing_url = configured_remote_url(sidecar_root, DEFAULT_REMOTE)?
        .or(remote_url(sidecar_root, DEFAULT_REMOTE)?);
    match existing_url.as_deref() {
        None => {
            run_git_checked(
                sidecar_root,
                &["remote", "add", DEFAULT_REMOTE, discovered_remote],
                "git remote add",
            )?;
        }
        Some(existing_url) if existing_url == discovered_remote => {}
        Some(existing_url) => {
            return Err(anyhow::Error::new(ExoFailure::new(
                ErrorCode::PreconditionFailed,
                format!(
                    "sidecar repo remote '{DEFAULT_REMOTE}' already points to '{existing_url}'; refusing to replace it with discovered remote '{discovered_remote}'"
                ),
                ExoFailure::orienting_steering(vec![SuggestedAction {
                    label: "Inspect sidecar remote".to_string(),
                    command: "exo sidecar repo remote --url <url> --replace".to_string(),
                    rationale: "Changing an existing sidecar remote requires explicit replacement."
                        .to_string(),
                    intent: WorkIntent::Orient,
                    confidence: Some(0.9),
                }]),
            )));
        }
    }

    fetch_remote(sidecar_root, DEFAULT_REMOTE)?;
    validate_discovered_sidecar_remote_history(sidecar_root, discovered_remote)
}

fn clone_discovered_sidecar_remote(sidecar_root: &Path, discovered_remote: &str) -> ExoResult<()> {
    if let Some(parent) = sidecar_root.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create sidecar parent directory {}",
                parent.display()
            )
        })?;
    }
    let output = ProcessCommand::new("git")
        .arg("clone")
        .arg(discovered_remote)
        .arg(sidecar_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .with_context(|| {
            format!("Failed to run git clone for discovered sidecar remote '{discovered_remote}'")
        })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let details = if stderr.is_empty() { stdout } else { stderr };
    anyhow::bail!(
        "git clone failed in {}: {details}",
        sidecar_root
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .display()
    )
}

fn directory_is_empty(path: &Path) -> ExoResult<bool> {
    let mut entries = std::fs::read_dir(path)
        .with_context(|| format!("Failed to read sidecar root {}", path.display()))?;
    Ok(entries.next().is_none())
}

fn validate_discovered_sidecar_remote_history(
    sidecar_root: &Path,
    discovered_remote: &str,
) -> ExoResult<()> {
    let Some(branch) = current_branch(sidecar_root)? else {
        return Err(discovered_sidecar_remote_history_error(
            sidecar_root,
            discovered_remote,
            "sidecar repo must be on a named branch before adopting a discovered remote",
        ));
    };
    let Some(relation) =
        read_upstream_relation_with_remote(sidecar_root, &branch, Some(DEFAULT_REMOTE))?
    else {
        return Err(discovered_sidecar_remote_history_error(
            sidecar_root,
            discovered_remote,
            &format!(
                "sidecar repo branch '{branch}' does not have a same-named '{DEFAULT_REMOTE}/{branch}' branch from the discovered remote"
            ),
        ));
    };
    if !relation.has_merge_base {
        return Err(discovered_sidecar_remote_history_error(
            sidecar_root,
            discovered_remote,
            "sidecar repo local branch and discovered remote have unrelated history; refusing to write sidecar state before recovery",
        ));
    }
    if relation.behind.unwrap_or(0) > 0 {
        if relation.ahead.unwrap_or(0) > 0 {
            return Err(discovered_sidecar_remote_history_error(
                sidecar_root,
                discovered_remote,
                "sidecar repo local branch and discovered remote have diverged; refusing to write sidecar state before recovery",
            ));
        }
        let remote_branch = format!("{DEFAULT_REMOTE}/{branch}");
        run_git_checked(
            sidecar_root,
            &["merge", "--ff-only", &remote_branch],
            "git merge --ff-only",
        )?;
    }
    Ok(())
}

fn discovered_sidecar_remote_history_error(
    sidecar_root: &Path,
    discovered_remote: &str,
    message: &str,
) -> anyhow::Error {
    anyhow::Error::new(ExoFailure::new(
        ErrorCode::PreconditionFailed,
        format!("{message} ({discovered_remote})"),
        ExoFailure::orienting_steering(vec![SuggestedAction {
            label: "Inspect sidecar repo recovery".to_string(),
            command: format!(
                "git -C {} status",
                shell_quote_arg(&sidecar_root.display().to_string())
            ),
            rationale: "Resolve the sidecar checkout before retrying discovery bootstrap."
                .to_string(),
            intent: WorkIntent::Orient,
            confidence: Some(0.9),
        }]),
    ))
}

fn expand_discovered_root(root: &str) -> ExoResult<PathBuf> {
    if root == "~" {
        return discovered_root_home_dir().map(PathBuf::from);
    }

    if let Some(relative) = root.strip_prefix("~/") {
        return discovered_root_home_dir().map(|home| PathBuf::from(home).join(relative));
    }

    Ok(PathBuf::from(root))
}

fn discovered_root_home_dir() -> ExoResult<PathBuf> {
    crate::project::platform_home_dir().ok_or_else(|| {
        anyhow::anyhow!(
            "discovered sidecar root uses ~ but no home directory is available; cannot expand sidecar root"
        )
    })
}

fn format_bootstrap_human_message(output: &SidecarBootstrapOutput) -> String {
    let mut message = format!(
        "Sidecar bootstrap ready at {}",
        output.sidecar_root.display()
    );
    message.push_str(&format!(
        "\nRoot source: {}",
        sidecar_root_source_label(output.sidecar_root_source)
    ));
    if let Some(default_sidecar_root) = &output.default_sidecar_root {
        message.push_str(&format!(
            "\nDefault user root: {}",
            default_sidecar_root.display()
        ));
    }
    let existing_project_roots: Vec<_> = output
        .known_sidecar_roots
        .iter()
        .filter(|root| !root.is_default)
        .collect();
    if !existing_project_roots.is_empty() {
        message.push_str("\nExisting project roots:");
        for root in existing_project_roots {
            let project_keys = if root.project_keys.is_empty() {
                "<none>".to_string()
            } else {
                root.project_keys.join(", ")
            };
            message.push_str(&format!("\n  {} ({project_keys})", root.root.display()));
        }
    }
    if let Some(discovery) = &output.discovery {
        message.push_str(&format!("\nDiscovery: {}", discovery.registry.label));
        if let Some(location) = discovery_registry_location(discovery) {
            message.push_str(&format!("\nDiscovery location: {location}"));
        }
        message.push_str(&format!(
            "\nSource: {} {}",
            discovery.identity.source,
            discovery.identity.login.as_deref().unwrap_or("<none>")
        ));
    }
    if let Some(issue) = &output.sync_issue {
        message.push_str(&format!("\nIssue: {issue}"));
    }
    if !output.next_actions.is_empty() {
        message.push_str("\nNext actions:");
        for action in &output.next_actions {
            message.push_str(&format!("\n  → {}", action.command));
        }
    }
    message
}

fn sidecar_root_source_label(source: &str) -> &'static str {
    match source {
        "explicit" => "explicit --root",
        "discovered" => "discovered registry root",
        "existing_project_root" => "existing project root",
        "default_user_root" => "default user sidecar root",
        _ => "unknown",
    }
}

fn sidecar_root_source(
    explicit_root: Option<&PathBuf>,
    discovered_root: Option<&PathBuf>,
    existing_sidecar_root: Option<&PathBuf>,
    selected_root: &Path,
    default_sidecar_root: Option<&Path>,
) -> &'static str {
    if explicit_root.is_some() {
        return "explicit";
    }
    if discovered_root.is_some() {
        return "discovered";
    }
    if existing_sidecar_root.is_some() {
        return "existing_project_root";
    }
    if default_sidecar_root
        .is_some_and(|default_sidecar_root| selected_root == default_sidecar_root)
    {
        return "default_user_root";
    }
    "default_user_root"
}

fn known_sidecar_roots(
    resolver: &ProjectResolver,
    default_sidecar_root: Option<&Path>,
) -> ExoResult<Vec<SidecarKnownRootOutput>> {
    let mut roots: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    if let Some(default_sidecar_root) = default_sidecar_root {
        roots.entry(default_sidecar_root.to_path_buf()).or_default();
    }

    let config_path = resolver.local_projects_config_path()?;
    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path).with_context(|| {
            format!("Failed to read project policy at {}", config_path.display())
        })?;
        let doc: toml::Value = toml::from_str(&content).with_context(|| {
            format!(
                "Failed to parse project policy at {}",
                config_path.display()
            )
        })?;
        if let Some(projects) = doc.get("projects").and_then(toml::Value::as_table) {
            for project in projects.values().filter_map(toml::Value::as_table) {
                if project.get("state").and_then(toml::Value::as_str) != Some("sidecar") {
                    continue;
                }
                let Some(root) = project.get("sidecar_root").and_then(toml::Value::as_str) else {
                    continue;
                };
                let key = project
                    .get("sidecar_key")
                    .and_then(toml::Value::as_str)
                    .unwrap_or("<missing-key>");
                roots
                    .entry(PathBuf::from(root))
                    .or_default()
                    .insert(key.to_string());
            }
        }
    }

    Ok(roots
        .into_iter()
        .map(|(root, project_keys)| {
            let is_default = default_sidecar_root.is_some_and(|default_root| root == default_root);
            SidecarKnownRootOutput {
                root,
                source: if is_default {
                    "default_user_root"
                } else {
                    "existing_project_root"
                },
                is_default,
                project_keys: project_keys.into_iter().collect(),
            }
        })
        .collect())
}

fn parse_auto_push_policy(value: &str) -> Option<SidecarAutoPushPolicy> {
    match value {
        "never" => Some(SidecarAutoPushPolicy::Never),
        "if_remote" => Some(SidecarAutoPushPolicy::IfRemote),
        "always" => Some(SidecarAutoPushPolicy::Always),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct SidecarInit {
    key: Option<String>,
    root: Option<PathBuf>,
    git: bool,
}

impl SidecarInit {
    pub const fn new(key: Option<String>, root: Option<PathBuf>, git: bool) -> Self {
        Self { key, root, git }
    }
}

#[derive(Debug, Serialize)]
struct SidecarInitOutput {
    kind: &'static str,
    ok: bool,
    project_id: String,
    sidecar_key: String,
    sidecar_root: PathBuf,
    config_path: PathBuf,
    manifest_path: PathBuf,
    projection_dir: PathBuf,
    db_path: PathBuf,
    db_created: bool,
    git_initialized: bool,
    seeded_from_repo: bool,
}

impl Command for SidecarInit {
    fn namespace(&self) -> &'static str {
        "sidecar"
    }

    fn operation(&self) -> &'static str {
        "init"
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let resolver = project_resolver_for_context(ctx.project);
        let link = init_sidecar_with_resolver(
            ctx.root,
            SidecarLinkOptions {
                key: self.key.clone(),
                root: self.root.clone(),
                auto_push: None,
                init_git: self.git,
                seed_from_repo: true,
            },
            &resolver,
        )?;
        let sidecar_key = link.sidecar_key.clone();
        let sidecar_root = link.sidecar_root.clone();
        let output = SidecarInitOutput {
            kind: "sidecar.init",
            ok: true,
            project_id: link.project.id.as_str().to_string(),
            sidecar_key: link.sidecar_key,
            sidecar_root: link.sidecar_root,
            config_path: link.config_path,
            manifest_path: link.manifest_path,
            projection_dir: link.projection_dir,
            db_path: link.db_path,
            db_created: link.db_created,
            git_initialized: link.git_initialized,
            seeded_from_repo: link.seeded_from_repo,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!(
                    "Sidecar ready: {} at {}",
                    sidecar_key,
                    sidecar_root.display()
                ),
            )),
        }
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn description(&self) -> &'static str {
        "Create or reuse sidecar state for this repo"
    }
}

#[derive(Debug, Clone)]
pub struct SidecarDiscover {
    registry_file: Option<PathBuf>,
}

impl SidecarDiscover {
    pub const fn new(registry_file: Option<PathBuf>) -> Self {
        Self { registry_file }
    }
}

#[derive(Debug, Serialize)]
struct SidecarDiscoveryRepositoryOutput {
    host: String,
    owner: String,
    repo: String,
    remote: String,
}

#[derive(Debug, Serialize)]
struct SidecarDiscoveryIdentityOutput {
    source: &'static str,
    login: Option<String>,
}

#[derive(Debug, Serialize)]
struct SidecarDiscoveryRegistryOutput {
    source: &'static str,
    label: String,
    profile_repo: Option<String>,
    path: Option<String>,
    version: Option<u8>,
}

#[derive(Debug, Serialize)]
struct SidecarDiscoveryMatchOutput {
    kind: &'static str,
    key: Option<String>,
}

#[derive(Debug, Serialize)]
struct SidecarDiscoveryProposalOutput {
    key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remote: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_push: Option<String>,
    would_mutate_config: bool,
    requires_remote_acceptance: bool,
}

#[derive(Debug, Serialize)]
struct SidecarDiscoveryFailureOutput {
    classification: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
}

#[derive(Debug, Serialize)]
struct SidecarDiscoveryCheckedAttemptOutput {
    attempt_index: usize,
    source: &'static str,
    identity_source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity_login: Option<String>,
    label: String,
    profile_repo: Option<String>,
    path: Option<String>,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug, Serialize)]
struct SidecarDiscoveryOutput {
    kind: &'static str,
    ok: bool,
    requires_git_repo: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    repository: Option<SidecarDiscoveryRepositoryOutput>,
    identity: SidecarDiscoveryIdentityOutput,
    registry: SidecarDiscoveryRegistryOutput,
    #[serde(rename = "match")]
    match_output: SidecarDiscoveryMatchOutput,
    confidence: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    proposal: Option<SidecarDiscoveryProposalOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<SidecarDiscoveryFailureOutput>,
    checked: Vec<SidecarDiscoveryCheckedAttemptOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attempt_index: Option<usize>,
    source_summary: String,
    next_actions: Vec<SuggestedAction>,
}

struct DiscoveryResolutionInput {
    repository: Option<SidecarDiscoveryRepositoryOutput>,
    identity: SidecarDiscoveryIdentityOutput,
    registry: SidecarDiscoveryRegistryOutput,
    failure_source: Option<String>,
    checked: Vec<SidecarDiscoveryCheckedAttemptOutput>,
    attempt_index: Option<usize>,
}

struct FailedDiscoveryInput {
    repository: Option<SidecarDiscoveryRepositoryOutput>,
    identity: SidecarDiscoveryIdentityOutput,
    registry: SidecarDiscoveryRegistryOutput,
    match_kind: &'static str,
    classification: &'static str,
    message: String,
    source: Option<String>,
    checked: Vec<SidecarDiscoveryCheckedAttemptOutput>,
    attempt_index: Option<usize>,
    next_actions: Vec<SuggestedAction>,
    requires_git_repo: bool,
}

impl Command for SidecarDiscover {
    fn namespace(&self) -> &'static str {
        "sidecar"
    }

    fn operation(&self) -> &'static str {
        "discover"
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let output = discover_sidecar(ctx.root, self.registry_file.as_deref());

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let message = format_discovery_human_message(&output);
                Ok(CommandOutput::new(output, message))
            }
        }
    }

    fn description(&self) -> &'static str {
        "Discover sidecar configuration for this repo"
    }
}

fn discover_sidecar(root: &Path, registry_file: Option<&Path>) -> SidecarDiscoveryOutput {
    if workspace_requires_git_init(root) {
        return failed_discovery_output(FailedDiscoveryInput {
            repository: None,
            identity: SidecarDiscoveryIdentityOutput {
                source: "unavailable",
                login: None,
            },
            registry: SidecarDiscoveryRegistryOutput {
                source: "none",
                label: "none".to_string(),
                profile_repo: None,
                path: None,
                version: None,
            },
            match_kind: "none",
            classification: "git-required",
            message: "This directory needs `git init` before sidecar bootstrap".to_string(),
            source: None,
            checked: Vec::new(),
            attempt_index: None,
            next_actions: vec![git_init_suggested_action()],
            requires_git_repo: true,
        });
    }

    let Some(remote) = git_origin_remote(root) else {
        return failed_discovery_output(FailedDiscoveryInput {
            repository: None,
            identity: SidecarDiscoveryIdentityOutput {
                source: "unavailable",
                login: None,
            },
            registry: SidecarDiscoveryRegistryOutput {
                source: "none",
                label: "none".to_string(),
                profile_repo: None,
                path: None,
                version: None,
            },
            match_kind: "none",
            classification: "no-github-remote",
            message: "No supported GitHub origin remote was found".to_string(),
            source: None,
            checked: Vec::new(),
            attempt_index: None,
            next_actions: vec![SuggestedAction {
                label: "Add workspace GitHub origin".to_string(),
                command: "git remote add origin <github-url>".to_string(),
                rationale: "Discovery needs the workspace repository's GitHub origin remote before it can infer owner/repo for profile registry lookup.".to_string(),
                intent: WorkIntent::Execute,
                confidence: Some(0.9),
            }],
            requires_git_repo: false,
        });
    };

    let repository = match parse_github_remote(&remote) {
        Ok(repository) => repository,
        Err(error) => {
            return failed_discovery_output(FailedDiscoveryInput {
                repository: None,
                identity: SidecarDiscoveryIdentityOutput {
                    source: "unavailable",
                    login: None,
                },
                registry: SidecarDiscoveryRegistryOutput {
                    source: "none",
                    label: "none".to_string(),
                    profile_repo: None,
                    path: None,
                    version: None,
                },
                match_kind: "none",
                classification: "no-github-remote",
                message: format!("Origin remote is not a supported GitHub remote: {error}"),
                source: None,
                checked: Vec::new(),
                attempt_index: None,
                next_actions: vec![SuggestedAction {
                    label: "Use a GitHub origin remote".to_string(),
                    command: "git remote set-url origin <github-url>".to_string(),
                    rationale:
                        "GitHub profile sidecar discovery starts from the workspace GitHub remote."
                            .to_string(),
                    intent: WorkIntent::Execute,
                    confidence: Some(0.8),
                }],
                requires_git_repo: false,
            });
        }
    };
    let repository_output = Some(repository_output(&repository));
    let identity = SidecarDiscoveryIdentityOutput {
        source: "remote-owner-unknown",
        login: Some(repository.owner.clone()),
    };

    discover_sidecar_with_fetcher(
        repository_output,
        identity,
        &repository,
        registry_file,
        &CurrentSidecarRegistryFetcher,
    )
}

fn discover_sidecar_with_fetcher(
    repository_output: Option<SidecarDiscoveryRepositoryOutput>,
    identity: SidecarDiscoveryIdentityOutput,
    repository: &ParsedGithubRemote,
    registry_file: Option<&Path>,
    fetcher: &impl SidecarRegistryFetcher,
) -> SidecarDiscoveryOutput {
    let request = SidecarRegistryFetchRequest::for_discovery(repository, registry_file);
    let fetch_report = fetcher.fetch(&request);
    resolve_fetched_registries(
        repository_output,
        identity,
        repository,
        registry_file,
        &request,
        fetch_report,
    )
}

fn resolve_fetched_registries(
    repository_output: Option<SidecarDiscoveryRepositoryOutput>,
    identity: SidecarDiscoveryIdentityOutput,
    repository: &ParsedGithubRemote,
    registry_file: Option<&Path>,
    request: &SidecarRegistryFetchRequest,
    mut fetch_report: SidecarRegistryFetchReport,
) -> SidecarDiscoveryOutput {
    let mut terminal_failure: Option<(SidecarRegistryFetchedRegistry, SidecarRegistryResolution)> =
        None;
    let mut last_no_match: Option<(SidecarRegistryFetchedRegistry, SidecarRegistryResolution)> =
        None;

    for fetched in &fetch_report.fetched {
        let resolution = resolve_sidecar_registry(&fetched.content, repository);
        let status = checked_status_for_resolution(&resolution);
        update_checked_attempt_resolution(
            &mut fetch_report.checked,
            fetched.attempt_index,
            status,
            resolution
                .failure
                .as_ref()
                .map(|failure| failure.message.clone()),
        );

        if resolution.ok {
            let checked = checked_attempt_outputs(&fetch_report.checked);
            return discovery_output_for_fetched_registry(
                repository_output,
                identity,
                repository,
                fetched,
                resolution,
                checked,
            );
        }

        if is_terminal_resolution_failure(&resolution) {
            terminal_failure = Some((fetched.clone(), resolution));
            break;
        }

        last_no_match = Some((fetched.clone(), resolution));
    }

    let checked = checked_attempt_outputs(&fetch_report.checked);
    if let Some((fetched, resolution)) = terminal_failure.or(last_no_match) {
        return discovery_output_for_fetched_registry(
            repository_output,
            identity,
            repository,
            &fetched,
            resolution,
            checked,
        );
    }

    discovery_output_for_fetch_failure(
        repository_output,
        identity,
        registry_file,
        request,
        &fetch_report,
        checked,
    )
}

fn discovery_output_for_fetched_registry(
    repository_output: Option<SidecarDiscoveryRepositoryOutput>,
    _identity: SidecarDiscoveryIdentityOutput,
    repository: &ParsedGithubRemote,
    fetched: &SidecarRegistryFetchedRegistry,
    resolution: SidecarRegistryResolution,
    checked: Vec<SidecarDiscoveryCheckedAttemptOutput>,
) -> SidecarDiscoveryOutput {
    let registry = registry_output_from_attempt(&fetched.attempt);
    let failure_source = fetched.attempt.source_locator();
    discovery_output_from_resolution(
        DiscoveryResolutionInput {
            repository: repository_output,
            identity: identity_output_from_attempt(&fetched.attempt),
            registry: SidecarDiscoveryRegistryOutput {
                version: resolution.ok.then_some(1),
                ..registry
            },
            failure_source,
            checked,
            attempt_index: Some(fetched.attempt_index),
        },
        repository,
        resolution,
    )
}

fn discovery_output_for_fetch_failure(
    repository_output: Option<SidecarDiscoveryRepositoryOutput>,
    identity: SidecarDiscoveryIdentityOutput,
    registry_file: Option<&Path>,
    request: &SidecarRegistryFetchRequest,
    fetch_report: &SidecarRegistryFetchReport,
    checked: Vec<SidecarDiscoveryCheckedAttemptOutput>,
) -> SidecarDiscoveryOutput {
    let selected_attempt = fetch_report
        .checked
        .iter()
        .rev()
        .find(|attempt| attempt.status != SidecarRegistryFetchStatus::Skipped)
        .or_else(|| fetch_report.checked.last());
    let registry = selected_attempt
        .map(|checked| registry_output_from_attempt(&checked.attempt))
        .or_else(|| request.attempts().first().map(registry_output_from_attempt))
        .unwrap_or_else(|| SidecarDiscoveryRegistryOutput {
            source: "none",
            label: "none".to_string(),
            profile_repo: None,
            path: None,
            version: None,
        });
    let failure_source = selected_attempt
        .and_then(|checked| checked.attempt.source_locator())
        .or_else(|| {
            request
                .attempts()
                .first()
                .and_then(SidecarRegistryFetchAttempt::source_locator)
        });
    let failure = selected_attempt
        .and_then(|attempt| attempt.message.clone())
        .unwrap_or_else(|| "No sidecar registry source produced a registry".to_string());
    let classification = fetch_failure_classification(registry_file, &fetch_report.checked);
    let identity = selected_attempt
        .map(|checked| identity_output_from_attempt(&checked.attempt))
        .unwrap_or(identity);
    let next_actions = if registry_file.is_some() {
        vec![SuggestedAction {
            label: "Inspect sidecar registry path".to_string(),
            command: "exo sidecar discover --registry-file <path>".to_string(),
            rationale: "Discovery could not read the requested registry file.".to_string(),
            intent: WorkIntent::Orient,
            confidence: Some(0.7),
        }]
    } else {
        vec![SuggestedAction {
            label: "Inspect sidecar profile registry".to_string(),
            command: "exo sidecar discover --verbose".to_string(),
            rationale: "Discovery checked GitHub profile registry sources but did not find a usable registry.".to_string(),
            intent: WorkIntent::Orient,
            confidence: Some(0.7),
        }]
    };

    failed_discovery_output(FailedDiscoveryInput {
        repository: repository_output,
        identity,
        registry,
        match_kind: "none",
        classification,
        message: failure,
        source: failure_source,
        checked,
        attempt_index: None,
        next_actions,
        requires_git_repo: false,
    })
}

fn fetch_failure_classification(
    registry_file: Option<&Path>,
    checked: &[SidecarRegistryCheckedAttempt],
) -> &'static str {
    if registry_file.is_some() {
        return "registry-fetch-error";
    }

    if checked
        .iter()
        .any(|attempt| attempt.status == SidecarRegistryFetchStatus::FetchError)
    {
        return "registry-fetch-error";
    }

    if checked
        .iter()
        .all(|attempt| attempt.status == SidecarRegistryFetchStatus::Skipped)
    {
        return "identity-unavailable";
    }

    "registry-not-found"
}

fn checked_status_for_resolution(
    resolution: &SidecarRegistryResolution,
) -> SidecarRegistryFetchStatus {
    if resolution.ok {
        return SidecarRegistryFetchStatus::LoadedMatch;
    }
    match resolution
        .failure
        .as_ref()
        .map(|failure| failure.classification)
    {
        Some(SidecarRegistryFailureClassification::NoMatchingEntry) => {
            SidecarRegistryFetchStatus::LoadedNoMatch
        }
        Some(SidecarRegistryFailureClassification::RegistryParseError) => {
            SidecarRegistryFetchStatus::ParseError
        }
        Some(SidecarRegistryFailureClassification::UnsafeRegistryValue) => {
            SidecarRegistryFetchStatus::UnsafeValue
        }
        None => SidecarRegistryFetchStatus::LoadedNoMatch,
    }
}

fn update_checked_attempt_resolution(
    checked: &mut [SidecarRegistryCheckedAttempt],
    attempt_index: usize,
    status: SidecarRegistryFetchStatus,
    message: Option<String>,
) {
    if let Some(attempt) = checked
        .iter_mut()
        .find(|attempt| attempt.attempt_index == attempt_index)
    {
        attempt.status = status;
        attempt.message = message;
    }
}

fn is_terminal_resolution_failure(resolution: &SidecarRegistryResolution) -> bool {
    matches!(
        resolution
            .failure
            .as_ref()
            .map(|failure| failure.classification),
        Some(
            SidecarRegistryFailureClassification::RegistryParseError
                | SidecarRegistryFailureClassification::UnsafeRegistryValue,
        )
    )
}

fn registry_output_from_attempt(
    attempt: &SidecarRegistryFetchAttempt,
) -> SidecarDiscoveryRegistryOutput {
    SidecarDiscoveryRegistryOutput {
        source: attempt.source().as_str(),
        label: attempt.label().to_string(),
        profile_repo: attempt.profile_repo().map(ToString::to_string),
        path: attempt.path().map(ToString::to_string),
        version: None,
    }
}

fn checked_attempt_outputs(
    attempts: &[SidecarRegistryCheckedAttempt],
) -> Vec<SidecarDiscoveryCheckedAttemptOutput> {
    attempts
        .iter()
        .map(|attempt| SidecarDiscoveryCheckedAttemptOutput {
            attempt_index: attempt.attempt_index,
            source: attempt.attempt.source().as_str(),
            identity_source: attempt.attempt.identity().source().as_str(),
            identity_login: attempt.attempt.identity().login().map(ToString::to_string),
            label: attempt.attempt.label().to_string(),
            profile_repo: attempt.attempt.profile_repo().map(ToString::to_string),
            path: attempt.attempt.path().map(ToString::to_string),
            status: attempt.status.as_str(),
            message: attempt.message.clone(),
        })
        .collect()
}

fn identity_output_from_attempt(
    attempt: &SidecarRegistryFetchAttempt,
) -> SidecarDiscoveryIdentityOutput {
    SidecarDiscoveryIdentityOutput {
        source: attempt.identity().source().as_str(),
        login: attempt.identity().login().map(ToString::to_string),
    }
}

fn discovery_output_from_resolution(
    input: DiscoveryResolutionInput,
    parsed_repository: &ParsedGithubRemote,
    resolution: SidecarRegistryResolution,
) -> SidecarDiscoveryOutput {
    let registry_file = (input.registry.source == "local-file")
        .then(|| input.registry.path.clone())
        .flatten();
    let next_actions = discovery_next_actions(&resolution, registry_file.as_deref());
    let match_key = (resolution.match_kind == SidecarRegistryMatchKind::ExactProject)
        .then(|| parsed_repository.project_key.clone());
    let source_summary = if resolution.ok {
        discovery_source_summary(
            &input.registry,
            resolution.match_kind.as_str(),
            match_key.as_deref(),
            resolution.confidence.as_str(),
        )
    } else {
        format!(
            "{} did not produce a usable sidecar discovery",
            input.registry.label
        )
    };
    let proposal = resolution.proposal.map(proposal_output);
    let failure = resolution
        .failure
        .map(|failure| SidecarDiscoveryFailureOutput {
            classification: failure.classification.as_str(),
            message: failure.message,
            source: input.failure_source,
        });

    SidecarDiscoveryOutput {
        kind: "sidecar.discovery",
        ok: resolution.ok,
        requires_git_repo: false,
        repository: input.repository,
        identity: input.identity,
        registry: input.registry,
        match_output: SidecarDiscoveryMatchOutput {
            kind: resolution.match_kind.as_str(),
            key: match_key,
        },
        confidence: resolution.confidence.as_str(),
        proposal,
        failure,
        checked: input.checked,
        attempt_index: input.attempt_index,
        source_summary,
        next_actions,
    }
}

fn failed_discovery_output(input: FailedDiscoveryInput) -> SidecarDiscoveryOutput {
    let source_summary = format!(
        "{} did not produce a usable sidecar discovery",
        input.registry.label
    );
    SidecarDiscoveryOutput {
        kind: "sidecar.discovery",
        ok: false,
        requires_git_repo: input.requires_git_repo,
        repository: input.repository,
        identity: input.identity,
        registry: input.registry,
        match_output: SidecarDiscoveryMatchOutput {
            kind: input.match_kind,
            key: None,
        },
        confidence: "none",
        proposal: None,
        failure: Some(SidecarDiscoveryFailureOutput {
            classification: input.classification,
            message: input.message,
            source: input.source,
        }),
        checked: input.checked,
        attempt_index: input.attempt_index,
        source_summary,
        next_actions: input.next_actions,
    }
}

fn discovery_source_summary(
    registry: &SidecarDiscoveryRegistryOutput,
    match_kind: &str,
    match_key: Option<&str>,
    confidence: &str,
) -> String {
    match match_key {
        Some(match_key) => format!(
            "{} matched {match_key} with {confidence} confidence",
            registry.label
        ),
        None => format!(
            "{} produced {match_kind} discovery with {confidence} confidence",
            registry.label
        ),
    }
}

fn repository_output(repository: &ParsedGithubRemote) -> SidecarDiscoveryRepositoryOutput {
    SidecarDiscoveryRepositoryOutput {
        host: repository.host.clone(),
        owner: repository.owner.clone(),
        repo: repository.repo.clone(),
        remote: repository.original.clone(),
    }
}

fn proposal_output(proposal: SidecarRegistryProposal) -> SidecarDiscoveryProposalOutput {
    SidecarDiscoveryProposalOutput {
        key: proposal.key,
        root: proposal.root,
        remote: proposal.remote,
        auto_push: proposal.auto_push,
        would_mutate_config: proposal.would_mutate_config,
        requires_remote_acceptance: proposal.requires_remote_acceptance,
    }
}

fn discovery_next_actions(
    resolution: &SidecarRegistryResolution,
    registry_file: Option<&str>,
) -> Vec<SuggestedAction> {
    if !resolution.ok {
        let command = registry_file.map_or_else(
            || "exo sidecar discover --verbose".to_string(),
            |path| {
                format!(
                    "exo sidecar discover --registry-file {} --verbose",
                    shell_quote_arg(path)
                )
            },
        );
        return vec![SuggestedAction {
            label: "Inspect sidecar registry".to_string(),
            command,
            rationale:
                "Review the registry source and discovery failure before applying configuration."
                    .to_string(),
            intent: WorkIntent::Orient,
            confidence: Some(0.7),
        }];
    }

    let Some(proposal) = &resolution.proposal else {
        return Vec::new();
    };

    if proposal.requires_remote_acceptance {
        vec![SuggestedAction {
            label: "Bootstrap and accept discovered remote".to_string(),
            command: "exo sidecar bootstrap --discover --accept-discovered-remote".to_string(),
            rationale:
                "Template/default discovery proposes a remote that requires explicit acceptance."
                    .to_string(),
            intent: WorkIntent::Execute,
            confidence: Some(discovery_confidence_score(resolution.confidence)),
        }]
    } else {
        vec![SuggestedAction {
            label: "Bootstrap from discovered sidecar config".to_string(),
            command: "exo sidecar bootstrap --discover".to_string(),
            rationale: "Exact project registry match can seed local sidecar configuration."
                .to_string(),
            intent: WorkIntent::Execute,
            confidence: Some(discovery_confidence_score(resolution.confidence)),
        }]
    }
}

const fn discovery_confidence_score(confidence: SidecarDiscoveryConfidence) -> f32 {
    match confidence {
        SidecarDiscoveryConfidence::High => 0.95,
        SidecarDiscoveryConfidence::Medium => 0.75,
        SidecarDiscoveryConfidence::Low => 0.55,
        SidecarDiscoveryConfidence::None => 0.0,
    }
}

fn format_discovery_human_message(output: &SidecarDiscoveryOutput) -> String {
    if output.ok {
        let mut message = "Discovered sidecar configuration".to_string();
        if let Some(repository) = &output.repository {
            message.push_str(&format!(
                "\nRepository: {}/{}/{}",
                repository.host, repository.owner, repository.repo
            ));
        }
        message.push_str(&format!("\nRegistry: {}", output.registry.label));
        if let Some(location) = discovery_registry_location(output) {
            message.push_str(&format!("\nRegistry location: {location}"));
        }
        if let Some(attempt_index) = output.attempt_index {
            message.push_str(&format!("\nRegistry attempt: {attempt_index}"));
        }
        message.push_str(&format!(
            "\nSource: {} {}",
            output.identity.source,
            output.identity.login.as_deref().unwrap_or("<none>")
        ));
        message.push_str(&format!(
            "\nMatch: {} ({})",
            output.match_output.kind, output.confidence
        ));
        message.push_str(&format!("\nConfidence: {}", output.confidence));
        if let Some(proposal) = &output.proposal {
            message.push_str("\nProposed sidecar:");
            message.push_str(&format!("\nKey: {}", proposal.key));
            if let Some(root) = &proposal.root {
                message.push_str(&format!("\nRoot: {root}"));
            }
            if let Some(remote) = &proposal.remote {
                message.push_str(&format!("\nRemote: {remote}"));
            }
            if let Some(auto_push) = &proposal.auto_push {
                message.push_str(&format!("\nAuto-push: {auto_push}"));
            }
            message.push_str(&format!(
                "\nWould mutate config: {}",
                proposal.would_mutate_config
            ));
            message.push_str(&format!(
                "\nRequires remote acceptance: {}",
                proposal.requires_remote_acceptance
            ));
        }
        if !output.next_actions.is_empty() {
            message.push_str("\nNext actions:");
            for action in &output.next_actions {
                message.push_str(&format!("\n  → {}", action.command));
            }
        }
        message
    } else {
        let mut message = "Sidecar discovery did not find a usable configuration".to_string();
        message.push_str(&format!("\nRegistry: {}", output.registry.label));
        if let Some(location) = discovery_registry_location(output) {
            message.push_str(&format!("\nRegistry location: {location}"));
        }
        if let Some(attempt_index) = output.attempt_index {
            message.push_str(&format!("\nRegistry attempt: {attempt_index}"));
        }
        if let Some(failure) = &output.failure {
            message.push_str(&format!("\nFailure: {}", failure.classification));
            message.push_str(&format!("\nMessage: {}", failure.message));
            if let Some(source) = &failure.source {
                message.push_str(&format!("\nSource: {source}"));
            }
        }
        if !output.next_actions.is_empty() {
            message.push_str("\nNext actions:");
            for action in &output.next_actions {
                message.push_str(&format!("\n  → {}", action.command));
            }
        }
        message
    }
}

fn discovery_registry_location(output: &SidecarDiscoveryOutput) -> Option<String> {
    output
        .registry
        .profile_repo
        .as_ref()
        .and_then(|profile_repo| {
            output
                .registry
                .path
                .as_ref()
                .map(|path| format!("{profile_repo}:{path}"))
        })
}

fn workspace_requires_git_init(root: &Path) -> bool {
    let Ok(output) = run_git(root, &["rev-parse", "--git-dir"]) else {
        return false;
    };
    !output.status.success() && git_required_for_sidecar(&output)
}

fn git_required_for_sidecar(output: &Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    stderr.contains("not a git repository")
        || stdout.contains("not a git repository")
        || stderr.contains("not in a git directory")
        || stdout.contains("not in a git directory")
}

fn git_init_suggested_action() -> SuggestedAction {
    SuggestedAction {
        label: "Initialize git repository".to_string(),
        command: "git init".to_string(),
        rationale:
            "Sidecar bootstrap needs a git project identity before it can bind portable state."
                .to_string(),
        intent: WorkIntent::Execute,
        confidence: Some(1.0),
    }
}

fn git_origin_remote(root: &Path) -> Option<String> {
    remote_url(root, "origin").ok().flatten()
}

#[derive(Debug, Clone)]
pub struct SidecarLink {
    key: String,
    root: PathBuf,
}

impl SidecarLink {
    pub fn new(key: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Self {
            key: key.into(),
            root: root.into(),
        }
    }
}

#[derive(Debug, Serialize)]
struct SidecarLinkOutput {
    kind: &'static str,
    ok: bool,
    project_id: String,
    sidecar_key: String,
    sidecar_root: PathBuf,
    config_path: PathBuf,
    manifest_path: PathBuf,
    projection_dir: PathBuf,
    db_path: PathBuf,
}

impl Command for SidecarLink {
    fn namespace(&self) -> &'static str {
        "sidecar"
    }

    fn operation(&self) -> &'static str {
        "link"
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let resolver = project_resolver_for_context(ctx.project);
        let link = link_sidecar_with_options_and_resolver(
            ctx.root, &self.key, &self.root, None, &resolver,
        )?;
        let output = SidecarLinkOutput {
            kind: "sidecar.link",
            ok: true,
            project_id: link.project.id.as_str().to_string(),
            sidecar_key: self.key.clone(),
            sidecar_root: self.root.clone(),
            config_path: link.config_path,
            manifest_path: link.manifest_path,
            projection_dir: link.projection_dir,
            db_path: link.db_path,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                format!("Linked sidecar '{}' at {}", self.key, self.root.display()),
            )),
        }
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn description(&self) -> &'static str {
        "Bind this repo to sidecar state"
    }
}

#[derive(Debug, Clone, Default)]
pub struct SidecarStatus {
    registry_file: Option<PathBuf>,
}

impl SidecarStatus {
    pub const fn new(registry_file: Option<PathBuf>) -> Self {
        Self { registry_file }
    }
}

#[derive(Debug, Serialize)]
struct SidecarStatusOutput {
    kind: &'static str,
    ok: bool,
    linked: bool,
    project_id: Option<String>,
    policy: Option<&'static str>,
    sidecar_key: Option<String>,
    sidecar_root: Option<PathBuf>,
    auto_commit: bool,
    auto_push: &'static str,
    manifest_path: Option<PathBuf>,
    projection_dir: Option<PathBuf>,
    db_path: Option<PathBuf>,
    runtime_dir: Option<PathBuf>,
    sidecar_repo: Option<SidecarRepoSyncStatus>,
    discovery: Option<SidecarDiscoveryOutput>,
    next_actions: Vec<SuggestedAction>,
}

impl Command for SidecarStatus {
    fn namespace(&self) -> &'static str {
        "sidecar"
    }

    fn operation(&self) -> &'static str {
        "status"
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let project = resolve_project_for_context(ctx.root, ctx.project)?;
        let linked = project.policy == StatePolicy::Sidecar;
        let sidecar_repo = linked
            .then(|| {
                let sidecar_root = project.sidecar_root.clone()?;
                let repo = ResolvedSidecarRepo {
                    project: project.clone(),
                    sidecar_root,
                };
                read_sidecar_repo_sync_status(&repo).ok()
            })
            .flatten();
        let discovery = linked
            .then(|| status_discovery(ctx.root, &project, self.registry_file.as_deref()))
            .flatten();
        let next_actions = status_next_actions(discovery.as_ref(), sidecar_repo.as_ref());
        let output = SidecarStatusOutput {
            kind: "sidecar.status",
            ok: sidecar_repo
                .as_ref()
                .is_none_or(|status| status.issue_kind != Some("unrelated_history")),
            linked,
            project_id: Some(project.id.as_str().to_string()),
            policy: Some(project.policy.as_str()),
            sidecar_key: project.sidecar_key.clone(),
            sidecar_root: project.sidecar_root.clone(),
            auto_commit: project.sidecar_auto_commit,
            auto_push: project.sidecar_auto_push.as_str(),
            manifest_path: project.sidecar_manifest_path(),
            projection_dir: project.sidecar_projection_dir(),
            db_path: linked.then(|| project.db_path()),
            runtime_dir: linked.then(|| project.runtime_dir()),
            sidecar_repo,
            discovery,
            next_actions,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let message = format_status_human_message(&output);
                Ok(CommandOutput::new(output, message))
            }
        }
    }

    fn description(&self) -> &'static str {
        "Show sidecar binding status"
    }
}

#[derive(Debug, Clone)]
pub struct SidecarSetup {
    dry_run: bool,
    profile_owner: Option<String>,
    state_repo: Option<String>,
    remote_url: Option<String>,
    replace_remote: bool,
}

impl SidecarSetup {
    pub const fn new(
        dry_run: bool,
        profile_owner: Option<String>,
        state_repo: Option<String>,
        remote_url: Option<String>,
        replace_remote: bool,
    ) -> Self {
        Self {
            dry_run,
            profile_owner,
            state_repo,
            remote_url,
            replace_remote,
        }
    }
}

#[derive(Debug, Serialize)]
struct SidecarSetupOutput {
    kind: &'static str,
    ok: bool,
    dry_run: bool,
    profile_owner: String,
    profile_repo: String,
    profile_path: &'static str,
    state_repo: String,
    remote_url: String,
    registry_entry: String,
    created_repo: bool,
    updated_registry: bool,
    configured_remote: bool,
}

impl Command for SidecarSetup {
    fn namespace(&self) -> &'static str {
        "sidecar"
    }

    fn operation(&self) -> &'static str {
        "setup"
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let project = resolve_project_for_context(ctx.root, ctx.project)?;
        let workspace_remote = first_remote_url(ctx.root)?.ok_or_else(|| {
            ExoFailure::new(
                ErrorCode::PreconditionFailed,
                "sidecar setup requires a GitHub remote on the workspace repository".to_string(),
                ExoFailure::orienting_steering(vec![SuggestedAction {
                    label: "Inspect git remotes".to_string(),
                    command: "git remote -v".to_string(),
                    rationale:
                        "Sidecar setup derives registry keys from the GitHub repository remote."
                            .to_string(),
                    intent: WorkIntent::Orient,
                    confidence: Some(1.0),
                }]),
            )
        })?;
        let repository = parse_github_remote(&workspace_remote).map_err(|error| {
            ExoFailure::new(
                ErrorCode::PreconditionFailed,
                format!("sidecar setup requires a GitHub remote: {error}"),
                ExoFailure::orienting_steering(vec![SuggestedAction {
                    label: "Inspect git remotes".to_string(),
                    command: "git remote -v".to_string(),
                    rationale: "Sidecar setup can only create GitHub profile registry entries for GitHub repositories."
                        .to_string(),
                    intent: WorkIntent::Orient,
                    confidence: Some(1.0),
                }]),
            )
        })?;

        let profile_owner = self
            .profile_owner
            .clone()
            .unwrap_or_else(|| repository.owner.clone());
        let state_repo = self
            .state_repo
            .clone()
            .unwrap_or_else(|| format!("{}-exosuit-state", repository.repo));
        let remote_url = self
            .remote_url
            .clone()
            .unwrap_or_else(|| format!("git@github.com:{profile_owner}/{state_repo}.git"));
        let sidecar_key = project
            .sidecar_key
            .clone()
            .unwrap_or_else(|| repository.repo.clone());

        let registry_entry = format!(
            "\n[projects.\"{}\"]\nkey = \"{}\"\nremote = \"{}\"\n",
            repository.project_key, sidecar_key, remote_url
        );

        let mut created_repo = false;
        let mut updated_registry = false;
        let mut configured_remote = false;
        if !self.dry_run {
            create_github_repo_if_missing(&profile_owner, &profile_owner)?;
            create_github_repo_if_missing(&profile_owner, &state_repo)?;
            created_repo = true;
            updated_registry = upsert_github_profile_registry(&profile_owner, &registry_entry)?;
            if let Some(sidecar_root) = &project.sidecar_root {
                apply_sidecar_setup_remote(sidecar_root, &remote_url, self.replace_remote)?;
                configured_remote = true;
            }
        }

        let output = SidecarSetupOutput {
            kind: "sidecar.setup",
            ok: true,
            dry_run: self.dry_run,
            profile_owner: profile_owner.clone(),
            profile_repo: format!("{profile_owner}/{profile_owner}"),
            profile_path: ".exosuit/sidecars.toml",
            state_repo,
            remote_url,
            registry_entry,
            created_repo,
            updated_registry,
            configured_remote,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                if self.dry_run {
                    "Sidecar setup plan generated".to_string()
                } else {
                    "Sidecar setup complete".to_string()
                },
            )),
        }
    }

    fn effect(&self) -> Effect {
        Effect::Exec
    }

    fn description(&self) -> &'static str {
        "Create GitHub sidecar repository and profile registry entry"
    }
}

fn format_status_human_message(output: &SidecarStatusOutput) -> String {
    let mut message = if output.linked {
        format!(
            "Sidecar linked: {}",
            output.sidecar_key.as_deref().unwrap_or("<missing>")
        )
    } else {
        "Sidecar not linked".to_string()
    };

    if let Some(discovery) = &output.discovery {
        message.push_str(&format!("\nDiscovery: {}", discovery.registry.label));
        if let Some(location) = discovery_registry_location(discovery) {
            message.push_str(&format!("\nDiscovery location: {location}"));
        }
        if let Some(attempt_index) = discovery.attempt_index {
            message.push_str(&format!("\nDiscovery attempt: {attempt_index}"));
        }
        message.push_str(&format!(
            "\nSource: {} {}",
            discovery.identity.source,
            discovery.identity.login.as_deref().unwrap_or("<none>")
        ));
        if let Some(proposal) = &discovery.proposal {
            message.push_str(&format!("\nCandidate: {}", proposal.key));
            if let Some(remote) = &proposal.remote {
                message.push_str(&format!("\nRemote: {remote}"));
            }
        }
        if let Some(failure) = &discovery.failure {
            message.push_str(&format!("\nDiscovery failure: {}", failure.classification));
            message.push_str(&format!("\nMessage: {}", failure.message));
        }
    }

    if let Some(sidecar_repo) = &output.sidecar_repo
        && let Some(issue) = &sidecar_repo.issue
    {
        if !sidecar_repo.ok || !sidecar_repo.foreign_checkpoint_debt.is_empty() {
            message.push_str(&format!("\nSidecar repo issue: {issue}"));
        }
    }

    if !output.next_actions.is_empty() {
        message.push_str("\nNext actions:");
        for action in &output.next_actions {
            message.push_str(&format!("\n  → {}", action.command));
        }
    }

    message
}

fn status_discovery(
    root: &Path,
    project: &Project,
    registry_file: Option<&Path>,
) -> Option<SidecarDiscoveryOutput> {
    let repo = project
        .sidecar_root
        .as_ref()
        .map(|sidecar_root| ResolvedSidecarRepo {
            project: project.clone(),
            sidecar_root: sidecar_root.clone(),
        })?;
    let repo_status = read_sidecar_repo_sync_status(&repo).ok()?;
    if repo_status.has_remote {
        return None;
    }
    Some(discover_sidecar(root, registry_file))
}

fn status_next_actions(
    discovery: Option<&SidecarDiscoveryOutput>,
    sidecar_repo: Option<&SidecarRepoSyncStatus>,
) -> Vec<SuggestedAction> {
    let mut actions = sidecar_repo
        .filter(|status| {
            status.issue_kind == Some("unrelated_history")
                || !status.foreign_checkpoint_debt.is_empty()
        })
        .map(sidecar_repo_sync_actions)
        .unwrap_or_default();
    let Some(discovery) = discovery else {
        return actions;
    };
    if !discovery.ok {
        actions.extend(discovery.next_actions.clone());
        return actions;
    }
    let Some(remote) = discovery
        .proposal
        .as_ref()
        .and_then(|proposal| proposal.remote.as_ref())
    else {
        actions.extend(discovery.next_actions.clone());
        return actions;
    };
    actions.push(SuggestedAction {
        label: "Add discovered sidecar remote".to_string(),
        command: format!("exo sidecar repo remote --url {remote}"),
        rationale: "Discovery found a concrete sidecar remote for this repository.".to_string(),
        intent: WorkIntent::Execute,
        confidence: Some(0.9),
    });
    actions
}

fn create_github_repo_if_missing(owner: &str, repo: &str) -> ExoResult<()> {
    let repo_ref = format!("{owner}/{repo}");
    let view = ProcessCommand::new("gh")
        .args(["repo", "view", &repo_ref])
        .output()?;
    if view.status.success() {
        return Ok(());
    }

    let create = ProcessCommand::new("gh")
        .args(["repo", "create", &repo_ref, "--private", "--confirm"])
        .output()?;
    if create.status.success() {
        return Ok(());
    }
    anyhow::bail!(
        "failed to create GitHub sidecar repo {repo_ref}: {}",
        String::from_utf8_lossy(&create.stderr).trim()
    )
}

fn upsert_github_profile_registry(profile_owner: &str, registry_entry: &str) -> ExoResult<bool> {
    let repo_ref = format!("{profile_owner}/{profile_owner}");
    let content = ProcessCommand::new("gh")
        .args([
            "api",
            &format!("repos/{repo_ref}/contents/.exosuit/sidecars.toml"),
        ])
        .output()?;

    let (mut registry, sha) = if content.status.success() {
        let metadata: serde_json::Value = serde_json::from_slice(&content.stdout)?;
        let download = ProcessCommand::new("gh")
            .args([
                "api",
                "-H",
                "Accept: application/vnd.github.raw",
                &format!("repos/{repo_ref}/contents/.exosuit/sidecars.toml"),
            ])
            .output()?;
        if !download.status.success() {
            anyhow::bail!(
                "failed to read existing GitHub profile sidecar registry: {}",
                String::from_utf8_lossy(&download.stderr).trim()
            );
        }
        (
            String::from_utf8_lossy(&download.stdout).to_string(),
            metadata
                .get("sha")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        )
    } else {
        (
            "version = 1\n\n[defaults]\nroot = \"~/.exo/sidecars\"\nauto_push = \"if_remote\"\n"
                .to_string(),
            None,
        )
    };
    let updated_registry = upsert_registry_entry(&registry, registry_entry)?;
    if updated_registry == registry {
        return Ok(false);
    }
    registry = updated_registry;

    let request_body = match sha {
        Some(sha) => format!(
            "{{\"message\":\"Update Exosuit sidecar registry\",\"content\":\"{}\",\"sha\":\"{}\"}}",
            base64_encode(registry.as_bytes()),
            json_escape(&sha)
        ),
        None => format!(
            "{{\"message\":\"Update Exosuit sidecar registry\",\"content\":\"{}\"}}",
            base64_encode(registry.as_bytes())
        ),
    };
    let temp = std::env::temp_dir().join(format!("exo-sidecars-{}.json", std::process::id()));
    std::fs::write(&temp, request_body)?;
    let endpoint = format!("repos/{repo_ref}/contents/.exosuit/sidecars.toml");
    let put = ProcessCommand::new("gh")
        .args([
            "api",
            "--method",
            "PUT",
            &endpoint,
            "--input",
            &temp.to_string_lossy(),
        ])
        .output()?;
    let _ = std::fs::remove_file(&temp);
    if put.status.success() {
        return Ok(true);
    }
    anyhow::bail!(
        "failed to update GitHub profile sidecar registry: {}",
        String::from_utf8_lossy(&put.stderr).trim()
    )
}

fn upsert_registry_entry(registry: &str, registry_entry: &str) -> ExoResult<String> {
    let document = registry
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| anyhow::anyhow!("failed to parse sidecar registry TOML: {error}"))?;
    let entry = registry_entry
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| {
            anyhow::anyhow!("failed to parse generated sidecar registry entry: {error}")
        })?;
    let projects = entry
        .get("projects")
        .and_then(toml_edit::Item::as_table)
        .ok_or_else(|| {
            anyhow::anyhow!("generated sidecar registry entry did not include projects table")
        })?;
    let project_key = projects
        .iter()
        .next()
        .map(|(key, _)| key.to_string())
        .ok_or_else(|| {
            anyhow::anyhow!("generated sidecar registry entry did not include a project")
        })?;
    let project = projects
        .get(&project_key)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("generated sidecar registry entry project missing"))?;

    let mut document = document;
    if !document
        .get("projects")
        .is_some_and(toml_edit::Item::is_table)
    {
        let mut table = toml_edit::Table::new();
        table.insert(&project_key, project);
        document["projects"] = toml_edit::Item::Table(table);
    } else {
        document["projects"][&project_key] = project;
    }
    Ok(document.to_string())
}

fn json_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(b0 >> 2) as usize] as char);
        output.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn apply_sidecar_setup_remote(
    sidecar_root: &Path,
    setup_remote_url: &str,
    replace_remote: bool,
) -> ExoResult<()> {
    ensure_git_repo(sidecar_root)?;
    if let Some(existing_url) = remote_url(sidecar_root, DEFAULT_REMOTE)? {
        if existing_url == setup_remote_url {
            return Ok(());
        }
        if !replace_remote {
            anyhow::bail!(
                "sidecar repo already has remote '{DEFAULT_REMOTE}'; rerun with --replace-remote to replace it"
            );
        }
        run_git_checked(
            sidecar_root,
            &["remote", "set-url", DEFAULT_REMOTE, setup_remote_url],
            "git remote set-url",
        )?;
    } else {
        run_git_checked(
            sidecar_root,
            &["remote", "add", DEFAULT_REMOTE, setup_remote_url],
            "git remote add",
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct SidecarRepo {
    action: SidecarRepoAction,
    message: Option<String>,
    remote: Option<String>,
    branch: Option<String>,
    url: Option<String>,
    replace: bool,
}

impl SidecarRepo {
    pub fn new(
        action: impl Into<String>,
        message: Option<String>,
        remote: Option<String>,
        branch: Option<String>,
        url: Option<String>,
        replace: bool,
    ) -> Self {
        Self {
            action: SidecarRepoAction::parse(action.into()),
            message,
            remote,
            branch,
            url,
            replace,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SidecarCheckpoint {
    project_key: Option<String>,
    message: Option<String>,
}

impl SidecarCheckpoint {
    pub fn new(project_key: Option<String>, message: Option<String>) -> Self {
        Self {
            project_key,
            message,
        }
    }
}

#[derive(Debug, Clone)]
enum SidecarRepoAction {
    Status,
    Commit,
    Remote,
    Push,
    Sync,
    Unknown(String),
}

impl SidecarRepoAction {
    fn parse(action: String) -> Self {
        match action.as_str() {
            "status" => Self::Status,
            "commit" => Self::Commit,
            "remote" => Self::Remote,
            "push" => Self::Push,
            "sync" => Self::Sync,
            _ => Self::Unknown(action),
        }
    }
}

#[derive(Debug, Serialize)]
struct SidecarRepoStatusOutput {
    kind: &'static str,
    ok: bool,
    sidecar_root: PathBuf,
    branch: Option<String>,
    clean: bool,
    repo_clean: bool,
    syncable: bool,
    has_remote: bool,
    remote: Option<String>,
    ahead: Option<u32>,
    behind: Option<u32>,
    issue_kind: Option<&'static str>,
    issue: Option<String>,
    ownership: SidecarWriteOwnershipStatus,
    files: Vec<SidecarRepoFileStatus>,
    project_files: Vec<SidecarRepoFileStatus>,
    foreign_checkpoint_debt: Vec<SidecarForeignCheckpointDebt>,
    next_actions: Vec<SuggestedAction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SidecarRepoSyncStatus {
    pub kind: &'static str,
    pub ok: bool,
    pub sidecar_root: PathBuf,
    pub branch: Option<String>,
    pub clean: bool,
    pub repo_clean: bool,
    pub syncable: bool,
    pub has_remote: bool,
    pub remote: Option<String>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub issue_kind: Option<&'static str>,
    pub issue: Option<String>,
    pub project_files: Vec<SidecarRepoFileStatus>,
    pub foreign_checkpoint_debt: Vec<SidecarForeignCheckpointDebt>,
}

#[derive(Debug, Clone)]
struct SidecarRepoIssue {
    kind: &'static str,
    message: String,
}

impl SidecarRepoIssue {
    fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SidecarAutoPersistReport {
    pub kind: &'static str,
    pub ok: bool,
    pub auto_commit: bool,
    pub auto_push: &'static str,
    pub committed: bool,
    pub commit: Option<String>,
    pub files_changed: usize,
    pub pushed: bool,
    pub remote: Option<String>,
    pub branch: Option<String>,
    pub issue: Option<String>,
}

#[derive(Debug, Serialize)]
struct SidecarRepoCommitOutput {
    kind: &'static str,
    ok: bool,
    sidecar_root: PathBuf,
    branch: Option<String>,
    committed: bool,
    commit: Option<String>,
    files_changed: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SidecarCheckpointOutput {
    pub kind: &'static str,
    pub ok: bool,
    pub sidecar_root: PathBuf,
    pub sidecar_key: String,
    pub git_backed: bool,
    pub branch: Option<String>,
    pub committed: bool,
    pub commit: Option<String>,
    pub files_changed: usize,
}

#[derive(Debug, Serialize)]
struct SidecarRepoRemoteOutput {
    kind: &'static str,
    ok: bool,
    sidecar_root: PathBuf,
    remote: String,
    url: String,
    previous_url: Option<String>,
    changed: bool,
    replaced: bool,
}

#[derive(Debug, Serialize)]
struct SidecarRepoPushOutput {
    kind: &'static str,
    ok: bool,
    sidecar_root: PathBuf,
    remote: String,
    branch: String,
    pushed: bool,
    already_synced: bool,
}

#[derive(Debug, Serialize)]
struct SidecarRepoSyncOutput {
    kind: &'static str,
    ok: bool,
    sidecar_root: PathBuf,
    branch: Option<String>,
    committed: bool,
    commit: Option<String>,
    merged: bool,
    pushed: bool,
    remote: Option<String>,
    issue_kind: Option<&'static str>,
    issue: Option<String>,
    conflicts: Vec<SqlProjectionConflict>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SidecarRepoFileStatus {
    pub path: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SidecarForeignCheckpointDebt {
    pub project: String,
    pub checkpointable: bool,
    pub issue_kind: Option<&'static str>,
    pub files: Vec<SidecarRepoFileStatus>,
    pub next_actions: Vec<SuggestedAction>,
}

#[derive(Debug, Clone, Serialize)]
struct SidecarWriteOwnershipStatus {
    kind: &'static str,
    ok: bool,
    state: &'static str,
    sidecar_key: Option<String>,
    marker_path: PathBuf,
    owner_pid: Option<u32>,
    owner_workspace_root: Option<PathBuf>,
    owner_db_path: Option<PathBuf>,
    owner_binary_blake3: Option<String>,
    issue: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SidecarWriteOwnerMarker {
    version: u32,
    sidecar_key: String,
    sidecar_root: PathBuf,
    workspace_root: Option<PathBuf>,
    state_root: PathBuf,
    db_path: PathBuf,
    runtime_dir: PathBuf,
    pid: u32,
    executable_path: Option<PathBuf>,
    executable_blake3: Option<String>,
    process_start_id: Option<String>,
    machine: String,
    acquired_at_ms: u128,
    refreshed_at_ms: u128,
}

impl Command for SidecarRepo {
    fn namespace(&self) -> &'static str {
        "sidecar"
    }

    fn operation(&self) -> &'static str {
        "repo"
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        match &self.action {
            SidecarRepoAction::Status => self.execute_status(ctx),
            SidecarRepoAction::Commit => self.execute_commit(ctx),
            SidecarRepoAction::Remote => self.execute_remote(ctx),
            SidecarRepoAction::Push => self.execute_push(ctx),
            SidecarRepoAction::Sync => self.execute_sync(ctx),
            SidecarRepoAction::Unknown(action) => Err(anyhow::Error::new(ExoFailure::new(
                ErrorCode::InvalidInput,
                format!(
                    "Unknown sidecar repo action '{action}'. Use status, commit, remote, push, or sync."
                ),
                ExoFailure::orienting_steering(vec![SuggestedAction {
                    label: "Inspect sidecar repo".to_string(),
                    command: "exo sidecar repo status".to_string(),
                    rationale: "Show the supported sidecar repo actions.".to_string(),
                    intent: WorkIntent::Orient,
                    confidence: Some(1.0),
                }]),
            ))),
        }
    }

    fn effect(&self) -> Effect {
        match self.action {
            SidecarRepoAction::Status => Effect::Pure,
            SidecarRepoAction::Commit
            | SidecarRepoAction::Remote
            | SidecarRepoAction::Push
            | SidecarRepoAction::Sync
            | SidecarRepoAction::Unknown(_) => Effect::Write,
        }
    }

    fn description(&self) -> &'static str {
        "Inspect, commit, configure, or push the sidecar git repo"
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        vec![SuggestedAction {
            label: "Check sidecar repo status".to_string(),
            command: "exo sidecar repo status".to_string(),
            rationale: "Inspect the sidecar git repository before retrying.".to_string(),
            intent: WorkIntent::Orient,
            confidence: Some(0.8),
        }]
    }
}

impl Command for SidecarCheckpoint {
    fn namespace(&self) -> &'static str {
        "sidecar"
    }

    fn operation(&self) -> &'static str {
        "checkpoint"
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let target =
            resolve_sidecar_checkpoint_target_for_context(ctx, self.project_key.as_deref())?;
        let message = self
            .message
            .as_deref()
            .unwrap_or("Checkpoint Exosuit sidecar state");

        crate::post_write::with_sidecar_runtime_lock(Some(&target.ownership_project), || {
            let output = checkpoint_resolved_sidecar_target(&target, message)?;
            match ctx.format {
                OutputFormat::Json => Ok(CommandOutput::data(output)),
                OutputFormat::Human => {
                    let message = if output.git_backed {
                        if output.committed {
                            format!(
                                "Checkpointed sidecar project {} at {} ({} file(s) changed)",
                                output.sidecar_key,
                                output.commit.as_deref().unwrap_or("<unknown>"),
                                output.files_changed
                            )
                        } else {
                            format!(
                                "Sidecar project {} already checkpointed.",
                                output.sidecar_key
                            )
                        }
                    } else {
                        format!(
                            "Flushed sidecar project {} projection (no git sidecar repo).",
                            output.sidecar_key
                        )
                    };
                    Ok(CommandOutput::new(output, message))
                }
            }
        })
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn description(&self) -> &'static str {
        "Create a local sidecar checkpoint for project state"
    }

    fn default_steering(&self) -> Vec<SuggestedAction> {
        vec![SuggestedAction::exo(
            "Check sidecar checkpoint state",
            ExoCommandReference::new(&["sidecar", "repo", "status"]),
            "Inspect sidecar checkpoint state before retrying.",
            WorkIntent::Orient,
            Some(0.8),
        )]
    }
}

impl SidecarRepo {
    fn execute_status(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let repo = resolve_sidecar_repo_for_context(ctx)?;
        let output = read_sidecar_repo_status(&repo)?;

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let state = if output.repo_clean { "clean" } else { "dirty" };
                let branch = output
                    .branch
                    .clone()
                    .unwrap_or_else(|| "<detached>".to_string());
                let mut message = format!(
                    "Sidecar repo {state} on {branch} at {}",
                    repo.sidecar_root.display()
                );
                if let Some(ownership) = human_sidecar_write_ownership(&output.ownership) {
                    message.push_str(&format!("\n{ownership}"));
                }
                if output.issue.is_some() || !output.next_actions.is_empty() {
                    if let Some(issue) = &output.issue {
                        message.push_str(&format!("\nIssue: {issue}"));
                    }
                    if !output.next_actions.is_empty() {
                        message.push_str("\nNext actions:");
                        for action in &output.next_actions {
                            message.push_str(&format!("\n  -> {}", action.command));
                        }
                    }
                }
                if !output.foreign_checkpoint_debt.is_empty() {
                    message.push_str("\nForeign checkpoint debt:");
                    for debt in &output.foreign_checkpoint_debt {
                        message.push_str(&format!(
                            "\n  - {}: {} file(s)",
                            debt.project,
                            debt.files.len()
                        ));
                        for action in &debt.next_actions {
                            message.push_str(&format!("\n    -> {}", action.command));
                        }
                    }
                }
                Ok(CommandOutput::new(output, message))
            }
        }
    }

    fn execute_commit(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let repo = resolve_sidecar_repo_for_context(ctx)?;
        let message = self.message.as_deref().ok_or_else(|| {
            ExoFailure::new(
                ErrorCode::MissingArg,
                "sidecar repo commit requires --message <msg>".to_string(),
                ExoFailure::orienting_steering(vec![SuggestedAction {
                    label: "Commit sidecar changes".to_string(),
                    command: "exo sidecar repo commit --message \"<msg>\"".to_string(),
                    rationale: "Provide a commit message for the sidecar repository.".to_string(),
                    intent: WorkIntent::Execute,
                    confidence: Some(1.0),
                }]),
            )
        })?;

        crate::post_write::with_sidecar_runtime_lock(Some(&repo.project), || {
            let checkpoint = commit_local_sidecar_checkpoint(&repo, message)?;
            if checkpoint.files_changed == 0 {
                let unowned_changes = unowned_sidecar_status_files(&repo)?.len();
                let output = SidecarRepoCommitOutput {
                    kind: "sidecar.repo.commit",
                    ok: true,
                    sidecar_root: repo.sidecar_root.clone(),
                    branch: checkpoint.branch,
                    committed: false,
                    commit: None,
                    files_changed: 0,
                };
                return match ctx.format {
                    OutputFormat::Json => Ok(CommandOutput::data(output)),
                    OutputFormat::Human => {
                        let message = if unowned_changes == 0 {
                            "Nothing to commit.".to_string()
                        } else {
                            format!(
                                "No owned sidecar changes to commit; {unowned_changes} unowned sidecar change(s) remain uncommitted."
                            )
                        };
                        Ok(CommandOutput::new(output, message))
                    }
                };
            }

            let output = SidecarRepoCommitOutput {
                kind: "sidecar.repo.commit",
                ok: true,
                sidecar_root: repo.sidecar_root.clone(),
                branch: checkpoint.branch,
                committed: checkpoint.committed,
                commit: checkpoint.commit.clone(),
                files_changed: checkpoint.files_changed,
            };

            match ctx.format {
                OutputFormat::Json => Ok(CommandOutput::data(output)),
                OutputFormat::Human => Ok(CommandOutput::new(
                    output,
                    format!(
                        "Committed sidecar repo {} ({} file(s) changed)",
                        checkpoint.commit.as_deref().unwrap_or("<unknown>"),
                        checkpoint.files_changed
                    ),
                )),
            }
        })
    }

    fn execute_remote(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let repo = resolve_sidecar_repo_for_context(ctx)?;
        let remote = self.remote.as_deref().unwrap_or("origin");
        let url = self.url.as_deref().ok_or_else(|| {
            ExoFailure::new(
                ErrorCode::MissingArg,
                "sidecar repo remote requires --url <url>".to_string(),
                ExoFailure::orienting_steering(vec![SuggestedAction {
                    label: "Add sidecar remote".to_string(),
                    command: "exo sidecar repo remote --url <url>".to_string(),
                    rationale: "Provide the git remote URL for portable sidecar state.".to_string(),
                    intent: WorkIntent::Execute,
                    confidence: Some(1.0),
                }]),
            )
        })?;

        let previous_url = remote_url(&repo.sidecar_root, remote)?;
        match previous_url.as_deref() {
            None => {
                run_git_checked(
                    &repo.sidecar_root,
                    &["remote", "add", remote, url],
                    "git remote add",
                )?;
            }
            Some(existing_url) if existing_url == url => {}
            Some(_) if self.replace => {
                run_git_checked(
                    &repo.sidecar_root,
                    &["remote", "set-url", remote, url],
                    "git remote set-url",
                )?;
            }
            Some(existing_url) => {
                return Err(anyhow::Error::new(ExoFailure::new(
                    ErrorCode::PreconditionFailed,
                    format!(
                        "sidecar repo remote '{remote}' already points to '{existing_url}'; use --replace to change it"
                    ),
                    ExoFailure::orienting_steering(vec![SuggestedAction {
                        label: "Replace sidecar remote".to_string(),
                        command: format!(
                            "exo sidecar repo remote --remote {remote} --url <url> --replace"
                        ),
                        rationale: "Changing an existing sidecar remote is explicit.".to_string(),
                        intent: WorkIntent::Execute,
                        confidence: Some(1.0),
                    }]),
                )));
            }
        }

        let changed = previous_url.as_deref() != Some(url);
        let replaced = previous_url.is_some() && changed;
        let output = SidecarRepoRemoteOutput {
            kind: "sidecar.repo.remote",
            ok: true,
            sidecar_root: repo.sidecar_root,
            remote: remote.to_string(),
            url: url.to_string(),
            previous_url,
            changed,
            replaced,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let message = if changed {
                    format!("Configured sidecar repo remote {remote} -> {url}")
                } else {
                    format!("Sidecar repo remote {remote} already points to {url}")
                };
                Ok(CommandOutput::new(output, message))
            }
        }
    }

    fn execute_push(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let repo = resolve_sidecar_repo_for_context(ctx)?;
        crate::post_write::with_sidecar_runtime_lock(Some(&repo.project), || {
            ensure_sidecar_write_ownership(&repo)?;
            let remote = self.remote.as_deref().unwrap_or("origin");
            ensure_remote_exists(&repo.sidecar_root, remote)?;
            let branch = match &self.branch {
                Some(branch) => branch.clone(),
                None => current_branch(&repo.sidecar_root)?.ok_or_else(|| {
                    ExoFailure::new(
                        ErrorCode::InvalidInput,
                        "sidecar repo push requires --branch when HEAD is detached".to_string(),
                        ExoFailure::orienting_steering(vec![SuggestedAction {
                            label: "Push a named branch".to_string(),
                            command: "exo sidecar repo push --branch <branch>".to_string(),
                            rationale: "Detached HEAD does not provide a branch name to push."
                                .to_string(),
                            intent: WorkIntent::Execute,
                            confidence: Some(1.0),
                        }]),
                    )
                })?,
            };

            let push = push_sidecar_branch_with_recovery(&repo, remote, &branch)?;

            let output = SidecarRepoPushOutput {
                kind: "sidecar.repo.push",
                ok: true,
                sidecar_root: repo.sidecar_root.clone(),
                remote: remote.to_string(),
                branch: branch.clone(),
                pushed: push.pushed,
                already_synced: push.already_synced,
            };
            match ctx.format {
                OutputFormat::Json => Ok(CommandOutput::data(output)),
                OutputFormat::Human => {
                    let message = if output.pushed {
                        format!("Pushed sidecar repo to {remote}/{branch}")
                    } else {
                        format!("Sidecar repo already synced with {remote}/{branch}")
                    };
                    Ok(CommandOutput::new(output, message))
                }
            }
        })
    }

    fn execute_sync(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let repo = resolve_sidecar_repo_for_context(ctx)?;
        crate::post_write::with_sidecar_runtime_lock(Some(&repo.project), || {
            let output = sync_resolved_sidecar_repo(&repo)?;

            match ctx.format {
                OutputFormat::Json => Ok(CommandOutput::data(output)),
                OutputFormat::Human => {
                    let message = if output.ok {
                        if output.merged {
                            "Synced sidecar repo with semantic SQL merge".to_string()
                        } else if output.pushed {
                            "Synced sidecar repo".to_string()
                        } else {
                            "Sidecar repo already synced".to_string()
                        }
                    } else if !output.conflicts.is_empty() {
                        format!(
                            "Sidecar sync needs review: conflicting {} row(s)",
                            output.conflicts.len()
                        )
                    } else {
                        output
                            .issue
                            .clone()
                            .unwrap_or_else(|| "Sidecar sync failed".to_string())
                    };
                    Ok(CommandOutput::new(output, message))
                }
            }
        })
    }
}

fn human_sidecar_write_ownership(ownership: &SidecarWriteOwnershipStatus) -> Option<String> {
    if ownership.ok {
        return None;
    }

    let issue = ownership.issue.as_deref().unwrap_or(ownership.state);
    if let Some(details) = issue.strip_prefix("sidecar write ownership marker is invalid: ") {
        return Some(format!("Ownership marker invalid: {details}"));
    }
    if let Some(details) = issue.strip_prefix("failed to read sidecar write ownership marker: ") {
        return Some(format!("Ownership marker unreadable: {details}"));
    }

    let owner = ownership
        .owner_pid
        .map(|pid| format!(" {pid}"))
        .unwrap_or_default();
    if issue.contains("active runtime") || issue.contains("live runtime") {
        Some(format!(
            "Ownership blocked by active runtime{owner}: {issue}"
        ))
    } else {
        Some(format!("Ownership blocked{owner}: {issue}"))
    }
}

pub fn sidecar_repo_sync_status(root: &Path) -> Option<SidecarRepoSyncStatus> {
    let project = Project::resolve(root).ok()?;
    if project.policy != StatePolicy::Sidecar {
        return None;
    }
    let sidecar_root = project.sidecar_root?;
    let repo = match resolve_sidecar_repo(root) {
        Ok(repo) => repo,
        Err(error) => {
            return Some(SidecarRepoSyncStatus {
                kind: "sidecar.repo.sync_status",
                ok: false,
                sidecar_root,
                branch: None,
                clean: false,
                repo_clean: false,
                syncable: false,
                has_remote: false,
                remote: None,
                ahead: None,
                behind: None,
                issue_kind: Some("invalid_repo"),
                issue: Some(error.to_string()),
                project_files: Vec::new(),
                foreign_checkpoint_debt: Vec::new(),
            });
        }
    };
    Some(
        read_sidecar_repo_sync_status(&repo).unwrap_or_else(|error| SidecarRepoSyncStatus {
            kind: "sidecar.repo.sync_status",
            ok: false,
            sidecar_root: repo.sidecar_root,
            branch: None,
            clean: false,
            repo_clean: false,
            syncable: false,
            has_remote: false,
            remote: None,
            ahead: None,
            behind: None,
            issue_kind: Some("invalid_repo"),
            issue: Some(error.to_string()),
            project_files: Vec::new(),
            foreign_checkpoint_debt: Vec::new(),
        }),
    )
}

fn bootstrap_next_actions(
    repo_status: Option<&SidecarRepoSyncStatus>,
    no_git: bool,
) -> Vec<SuggestedAction> {
    if no_git {
        return vec![SuggestedAction {
            label: "Initialize sidecar git repo".to_string(),
            command: "exo sidecar bootstrap".to_string(),
            rationale:
                "Re-run bootstrap without --no-git when you are ready to version sidecar state."
                    .to_string(),
            intent: WorkIntent::Execute,
            confidence: Some(0.9),
        }];
    }

    let Some(status) = repo_status else {
        return Vec::new();
    };
    let mut actions = Vec::new();
    if !status.foreign_checkpoint_debt.is_empty() {
        actions.extend(sidecar_checkpoint_debt_actions(
            &status.foreign_checkpoint_debt,
        ));
    }
    if !status.clean {
        actions.push(SuggestedAction {
            label: "Commit sidecar state".to_string(),
            command: "exo sidecar repo commit --message \"Bootstrap Exosuit sidecar state\""
                .to_string(),
            rationale: "Save the bootstrapped sidecar projection in the sidecar git repository."
                .to_string(),
            intent: WorkIntent::Execute,
            confidence: Some(1.0),
        });
    } else if !status.repo_clean {
        let issue_kind = match status.issue_kind.as_deref() {
            Some("no_remote") => "no_remote",
            Some("behind") => "behind",
            Some("ahead") => "ahead",
            Some("no_upstream") => "no_upstream",
            Some("unrelated_history") => "unrelated_history",
            _ => "dirty",
        };
        actions.extend(sidecar_repo_issue_actions(issue_kind));
    }
    if !status.has_remote {
        actions.push(SuggestedAction {
            label: "Add sidecar remote".to_string(),
            command: "exo sidecar repo remote --url <url>".to_string(),
            rationale:
                "Bootstrap does not create remotes; configure one before pushing portable sidecar state."
                    .to_string(),
            intent: WorkIntent::Execute,
            confidence: Some(1.0),
        });
    }
    if status.syncable && status.behind.unwrap_or(0) > 0 {
        actions.push(SuggestedAction {
            label: "Sync sidecar repo".to_string(),
            command: "exo sidecar repo sync".to_string(),
            rationale: "Merge portable sidecar state semantically before pushing new state."
                .to_string(),
            intent: WorkIntent::Execute,
            confidence: Some(0.8),
        });
    }
    if status.behind.unwrap_or(0) == 0
        && status
            .issue
            .as_deref()
            .is_some_and(|issue| issue.contains("not been pushed"))
    {
        actions.push(SuggestedAction {
            label: "Push sidecar repo".to_string(),
            command: "exo sidecar repo push".to_string(),
            rationale: "Publish the committed sidecar state to the configured remote.".to_string(),
            intent: WorkIntent::Execute,
            confidence: Some(0.9),
        });
    }
    actions
}

pub fn auto_persist_after_successful_mutation(root: &Path) -> Option<SidecarAutoPersistReport> {
    let repo = resolve_sidecar_repo(root).ok()?;
    Some(auto_persist_resolved_sidecar_repo(&repo))
}

pub fn auto_persist_after_successful_mutation_with_project(
    project: &Project,
) -> Option<SidecarAutoPersistReport> {
    if project.policy != StatePolicy::Sidecar {
        return None;
    }
    let sidecar_root = project.sidecar_root.clone()?;
    if ensure_git_repo(&sidecar_root).is_err() {
        return None;
    }
    let repo = ResolvedSidecarRepo {
        project: project.clone(),
        sidecar_root,
    };
    Some(auto_persist_resolved_sidecar_repo(&repo))
}

pub fn checkpoint_after_successful_mutation_with_project(
    project: &Project,
) -> ExoResult<Option<SidecarAutoPersistReport>> {
    if project.policy != StatePolicy::Sidecar {
        return Ok(None);
    }
    let sidecar_root = project.sidecar_root.clone().ok_or_else(|| {
        ExoFailure::new(
            ErrorCode::PreconditionFailed,
            "sidecar checkpoint requires a sidecar root".to_string(),
            ExoFailure::orienting_steering(vec![SuggestedAction::exo(
                "Relink sidecar state",
                ExoCommandReference::new(&["sidecar", "link"])
                    .option("key", "<key>")
                    .option("root", "<sidecar-root>"),
                "Repair the local sidecar policy with an explicit root.",
                WorkIntent::Execute,
                Some(1.0),
            )]),
        )
    })?;

    if is_independent_git_repo(&sidecar_root)? {
        let repo = ResolvedSidecarRepo {
            project: project.clone(),
            sidecar_root,
        };
        return Ok(Some(auto_persist_resolved_sidecar_repo_result(&repo)?));
    }

    let target = ResolvedSidecarCheckpointTarget {
        project: project.clone(),
        ownership_project: project.clone(),
        sidecar_root,
        git_backed: false,
        flush_projection: true,
    };
    let checkpoint =
        checkpoint_resolved_sidecar_target(&target, "Checkpoint Exosuit sidecar state")?;
    Ok(Some(SidecarAutoPersistReport {
        kind: "sidecar.auto_persist",
        ok: true,
        auto_commit: false,
        auto_push: project.sidecar_auto_push.as_str(),
        committed: checkpoint.committed,
        commit: checkpoint.commit,
        files_changed: checkpoint.files_changed,
        pushed: false,
        remote: None,
        branch: checkpoint.branch,
        issue: None,
    }))
}

pub fn ensure_sidecar_write_ownership_for_project(project: &Project) -> ExoResult<()> {
    if project.policy != StatePolicy::Sidecar {
        return Ok(());
    }
    let sidecar_root = project
        .sidecar_root
        .clone()
        .ok_or_else(|| anyhow::anyhow!("sidecar write ownership requires a sidecar root"))?;
    let repo = ResolvedSidecarRepo {
        project: project.clone(),
        sidecar_root,
    };
    ensure_sidecar_write_ownership(&repo).map(|_| ())
}

pub fn sidecar_write_ownership_applies_to_project(project: &Project) -> bool {
    if project.policy != StatePolicy::Sidecar {
        return false;
    }
    let Some(sidecar_root) = project.sidecar_root.as_deref() else {
        return false;
    };
    is_independent_git_repo(sidecar_root).unwrap_or(false)
}

fn auto_persist_resolved_sidecar_repo(repo: &ResolvedSidecarRepo) -> SidecarAutoPersistReport {
    if !repo.project.sidecar_auto_commit {
        return SidecarAutoPersistReport {
            kind: "sidecar.auto_persist",
            ok: true,
            auto_commit: false,
            auto_push: repo.project.sidecar_auto_push.as_str(),
            committed: false,
            commit: None,
            files_changed: 0,
            pushed: false,
            remote: None,
            branch: None,
            issue: None,
        };
    }

    match auto_persist_resolved_sidecar_repo_result(repo) {
        Ok(report) => report,
        Err(error) => SidecarAutoPersistReport {
            kind: "sidecar.auto_persist",
            ok: false,
            auto_commit: repo.project.sidecar_auto_commit,
            auto_push: repo.project.sidecar_auto_push.as_str(),
            committed: false,
            commit: None,
            files_changed: 0,
            pushed: false,
            remote: None,
            branch: None,
            issue: Some(error.to_string()),
        },
    }
}

struct ResolvedSidecarRepo {
    project: Project,
    sidecar_root: PathBuf,
}

struct ResolvedSidecarCheckpointTarget {
    project: Project,
    ownership_project: Project,
    sidecar_root: PathBuf,
    git_backed: bool,
    flush_projection: bool,
}

struct LocalSidecarCheckpoint {
    branch: Option<String>,
    committed: bool,
    commit: Option<String>,
    files_changed: usize,
}

fn resolve_sidecar_checkpoint_target_for_context(
    ctx: &CommandContext,
    project_key: Option<&str>,
) -> ExoResult<ResolvedSidecarCheckpointTarget> {
    let current_project = match ctx.project {
        Some(project) => project.clone(),
        None => resolve_project_for_context(ctx.root, None)?,
    };
    if current_project.policy != StatePolicy::Sidecar {
        return Err(anyhow::Error::new(ExoFailure::new(
            ErrorCode::PreconditionFailed,
            "sidecar checkpoint requires active sidecar policy".to_string(),
            ExoFailure::orienting_steering(vec![SuggestedAction::exo(
                "Initialize sidecar state",
                ExoCommandReference::new(&["sidecar", "init"]).flag("git"),
                "Bind this project to sidecar state before checkpointing.",
                WorkIntent::Execute,
                Some(1.0),
            )]),
        )));
    }

    let sidecar_root = current_project.sidecar_root.clone().ok_or_else(|| {
        ExoFailure::new(
            ErrorCode::PreconditionFailed,
            "active sidecar policy is missing sidecar_root".to_string(),
            ExoFailure::orienting_steering(vec![SuggestedAction::exo(
                "Relink sidecar state",
                ExoCommandReference::new(&["sidecar", "link"])
                    .option("key", "<key>")
                    .option("root", "<sidecar-root>"),
                "Repair the local sidecar policy with an explicit root.",
                WorkIntent::Execute,
                Some(1.0),
            )]),
        )
    })?;

    let git_backed = is_independent_git_repo(&sidecar_root)?;
    let (project, flush_projection) = match project_key {
        Some(key) => {
            validate_sidecar_checkpoint_project_key(key)?;
            if current_project.sidecar_key.as_deref() == Some(key) {
                return Ok(ResolvedSidecarCheckpointTarget {
                    project: current_project.clone(),
                    ownership_project: current_project,
                    sidecar_root,
                    git_backed,
                    flush_projection: true,
                });
            }
            let state_root = sidecar_root.join("projects").join(key);
            let has_checkpoint_debt =
                git_backed && sidecar_project_has_status_debt(&sidecar_root, key)?;
            if !state_root.exists() && !has_checkpoint_debt {
                return Err(anyhow::Error::new(
                    ExoFailure::new(
                        ErrorCode::NotFound,
                        format!(
                            "sidecar project '{key}' does not exist under {}",
                            sidecar_root.display()
                        ),
                        ExoFailure::orienting_steering(vec![SuggestedAction::exo(
                            "Show sidecar repo status",
                            ExoCommandReference::new(&["sidecar", "repo"]).positional("status"),
                            "Inspect sidecar projects before checkpointing a named project.",
                            WorkIntent::Orient,
                            Some(0.8),
                        )]),
                    )
                    .with_details(serde_json::json!({
                        "kind": "sidecar.checkpoint",
                        "sidecar_key": key,
                        "sidecar_root": sidecar_root,
                        "state_root": state_root,
                    })),
                ));
            }
            (
                Project {
                    id: crate::project::ProjectId::from_git_common_dir(&state_root),
                    git_common_dir: current_project.git_common_dir.clone(),
                    workspace_root: None,
                    policy: StatePolicy::Sidecar,
                    projects_config_path: current_project.projects_config_path.clone(),
                    state_root,
                    sidecar_key: Some(key.to_string()),
                    sidecar_root: Some(sidecar_root.clone()),
                    sidecar_auto_commit: current_project.sidecar_auto_commit,
                    sidecar_auto_push: current_project.sidecar_auto_push,
                },
                false,
            )
        }
        None => (current_project.clone(), true),
    };

    Ok(ResolvedSidecarCheckpointTarget {
        project,
        ownership_project: current_project,
        sidecar_root,
        git_backed,
        flush_projection,
    })
}

fn validate_sidecar_checkpoint_project_key(key: &str) -> ExoResult<()> {
    let path = Path::new(key);
    let mut has_component = false;
    let valid = !path.is_absolute()
        && !key.contains('\\')
        && path.components().all(|component| match component {
            Component::Normal(component) if !component.is_empty() => {
                has_component = true;
                true
            }
            _ => false,
        })
        && has_component;
    if valid {
        return Ok(());
    }

    Err(anyhow::Error::new(
        ExoFailure::new(
            ErrorCode::InvalidInput,
            format!(
                "sidecar checkpoint project key must be a safe relative project key, got '{key}'"
            ),
            ExoFailure::orienting_steering(vec![SuggestedAction::exo(
                "Inspect sidecar repo status",
                ExoCommandReference::new(&["sidecar", "repo", "status"]),
                "Use a project key shown in sidecar repo status.",
                WorkIntent::Orient,
                Some(0.9),
            )]),
        )
        .with_details(serde_json::json!({
            "kind": "sidecar.checkpoint",
            "sidecar_key": key,
            "issue": "invalid_project_key",
        })),
    ))
}

fn checkpoint_resolved_sidecar_target(
    target: &ResolvedSidecarCheckpointTarget,
    message: &str,
) -> ExoResult<SidecarCheckpointOutput> {
    let sidecar_key = target.project.sidecar_key.clone().ok_or_else(|| {
        anyhow::anyhow!("sidecar checkpoint requires the current project sidecar key")
    })?;

    if target.git_backed {
        let repo = ResolvedSidecarRepo {
            project: target.project.clone(),
            sidecar_root: target.sidecar_root.clone(),
        };
        let checkpoint = commit_local_sidecar_checkpoint_with_options(
            &repo,
            message,
            &target.ownership_project,
            target.flush_projection,
        )?;
        return Ok(SidecarCheckpointOutput {
            kind: "sidecar.checkpoint",
            ok: true,
            sidecar_root: target.sidecar_root.clone(),
            sidecar_key,
            git_backed: true,
            branch: checkpoint.branch,
            committed: checkpoint.committed,
            commit: checkpoint.commit,
            files_changed: checkpoint.files_changed,
        });
    }

    if target.flush_projection {
        crate::context::write_sql_dump_with_project_result(
            &target.project_workspace_root(),
            Some(&target.project),
        )?;
    } else if !target.project.db_path().exists() {
        return Err(anyhow::Error::new(
            ExoFailure::new(
                ErrorCode::PreconditionFailed,
                "named sidecar checkpoint requires a git-backed sidecar or an existing project cache database".to_string(),
                ExoFailure::orienting_steering(vec![SuggestedAction::exo(
                    "Inspect sidecar repo status",
                    ExoCommandReference::new(&["sidecar", "repo", "status"]),
                    "Review named sidecar checkpoint debt before retrying.",
                    WorkIntent::Orient,
                    Some(0.9),
                )]),
            )
            .with_details(serde_json::json!({
                "kind": "sidecar.checkpoint",
                "sidecar_key": sidecar_key,
                "sidecar_root": target.sidecar_root,
                "state_root": target.project.state_root,
                "db_path": target.project.db_path(),
                "issue": "named no-git sidecar checkpoint cannot synthesize projection without a cache DB",
            })),
        ));
    } else {
        crate::context::write_sql_dump_with_project_result(
            &target.project_workspace_root(),
            Some(&target.project),
        )?;
    }
    Ok(SidecarCheckpointOutput {
        kind: "sidecar.checkpoint",
        ok: true,
        sidecar_root: target.sidecar_root.clone(),
        sidecar_key,
        git_backed: false,
        branch: None,
        committed: false,
        commit: None,
        files_changed: 0,
    })
}

fn commit_local_sidecar_checkpoint(
    repo: &ResolvedSidecarRepo,
    message: &str,
) -> ExoResult<LocalSidecarCheckpoint> {
    let ownership_project = repo.project.clone();
    commit_local_sidecar_checkpoint_with_options(repo, message, &ownership_project, true)
}

fn commit_local_sidecar_checkpoint_with_options(
    repo: &ResolvedSidecarRepo,
    message: &str,
    ownership_project: &Project,
    flush_projection: bool,
) -> ExoResult<LocalSidecarCheckpoint> {
    let ownership_repo = ResolvedSidecarRepo {
        project: ownership_project.clone(),
        sidecar_root: repo.sidecar_root.clone(),
    };
    ensure_sidecar_write_ownership(&ownership_repo)?;
    crate::project::ensure_sidecar_git_identity(&repo.sidecar_root)?;
    ensure_foreign_sidecar_checkpoint_target_available(repo, ownership_project)?;
    if flush_projection {
        crate::context::write_sql_dump_with_project_result(
            &repo.project_workspace_root(),
            Some(&repo.project),
        )?;
    }
    ensure_sidecar_runtime_gitignore(&repo.sidecar_root)?;
    ensure_sidecar_runtime_paths_untracked(repo)?;
    ensure_no_unowned_staged_sidecar_paths(repo)?;

    let files_changed = owned_sidecar_status_files(repo)?.len();
    let branch = current_branch(&repo.sidecar_root)?;
    if files_changed == 0 {
        return Ok(LocalSidecarCheckpoint {
            branch,
            committed: false,
            commit: None,
            files_changed,
        });
    }

    stage_owned_sidecar_paths(repo)?;
    ensure_no_unowned_staged_sidecar_paths(repo)?;
    run_git_checked(&repo.sidecar_root, &["commit", "-m", message], "git commit")?;
    let commit = git_stdout(&run_git_checked(
        &repo.sidecar_root,
        &["rev-parse", "--short", "HEAD"],
        "git rev-parse --short HEAD",
    )?);

    Ok(LocalSidecarCheckpoint {
        branch,
        committed: true,
        commit: Some(commit),
        files_changed,
    })
}

fn ensure_foreign_sidecar_checkpoint_target_available(
    repo: &ResolvedSidecarRepo,
    ownership_project: &Project,
) -> ExoResult<()> {
    if repo.project.sidecar_key == ownership_project.sidecar_key {
        return Ok(());
    }

    let status = read_sidecar_write_ownership_status(repo)?;
    if status.ok {
        return Ok(());
    }

    let issue = status.issue.clone().unwrap_or_else(|| {
        "foreign sidecar project checkpoint target is owned by another active runtime".to_string()
    });
    Err(anyhow::Error::new(
        ExoFailure::new(
            ErrorCode::PreconditionFailed,
            issue,
            ExoFailure::orienting_steering(vec![SuggestedAction::exo(
                "Inspect sidecar repo status",
                ExoCommandReference::new(&["sidecar", "repo", "status"]),
                "Review sidecar checkpoint ownership before retrying.",
                WorkIntent::Orient,
                Some(1.0),
            )]),
        )
        .with_details(serde_json::json!({
            "kind": "sidecar.checkpoint",
            "sidecar_key": repo.project.sidecar_key.clone(),
            "sidecar_root": repo.sidecar_root.clone(),
            "ownership_state": status.state,
            "owner_pid": status.owner_pid,
            "owner_workspace_root": status.owner_workspace_root,
        })),
    ))
}

fn resolve_sidecar_repo_for_context(ctx: &CommandContext) -> ExoResult<ResolvedSidecarRepo> {
    match ctx.project {
        Some(project) => resolve_sidecar_repo_from_project(project),
        None => resolve_sidecar_repo(ctx.root),
    }
}

fn resolve_sidecar_repo(root: &Path) -> ExoResult<ResolvedSidecarRepo> {
    let project = Project::resolve(root)?;
    resolve_sidecar_repo_from_project(&project)
}

fn resolve_sidecar_repo_from_project(project: &Project) -> ExoResult<ResolvedSidecarRepo> {
    if project.policy != StatePolicy::Sidecar {
        return Err(anyhow::Error::new(ExoFailure::new(
            ErrorCode::PreconditionFailed,
            "sidecar repo commands require active sidecar policy".to_string(),
            ExoFailure::orienting_steering(vec![SuggestedAction {
                label: "Initialize sidecar state".to_string(),
                command: "exo sidecar init --git".to_string(),
                rationale: "Bind this project to a sidecar before managing its repository."
                    .to_string(),
                intent: WorkIntent::Execute,
                confidence: Some(1.0),
            }]),
        )));
    }

    let sidecar_root = project.sidecar_root.clone().ok_or_else(|| {
        ExoFailure::new(
            ErrorCode::PreconditionFailed,
            "active sidecar policy is missing sidecar_root".to_string(),
            ExoFailure::orienting_steering(vec![SuggestedAction {
                label: "Relink sidecar state".to_string(),
                command: "exo sidecar link --key <key> --root <sidecar-root>".to_string(),
                rationale: "Repair the local sidecar policy with an explicit root.".to_string(),
                intent: WorkIntent::Execute,
                confidence: Some(1.0),
            }]),
        )
    })?;
    ensure_git_repo(&sidecar_root)?;
    Ok(ResolvedSidecarRepo {
        project: project.clone(),
        sidecar_root,
    })
}

fn ensure_git_repo(root: &Path) -> ExoResult<()> {
    if is_independent_git_repo(root)? {
        return Ok(());
    }

    Err(anyhow::Error::new(ExoFailure::new(
        ErrorCode::PreconditionFailed,
        format!(
            "sidecar root {} is not a git repository; run `exo sidecar init --git` or initialize it with git",
            root.display()
        ),
        ExoFailure::orienting_steering(vec![SuggestedAction {
            label: "Initialize sidecar git repo".to_string(),
            command: "exo sidecar init --git".to_string(),
            rationale: "Create the sidecar repository before committing or pushing.".to_string(),
            intent: WorkIntent::Execute,
            confidence: Some(1.0),
        }]),
    )))
}

fn is_independent_git_repo(root: &Path) -> ExoResult<bool> {
    let output = run_git(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    if !output.status.success() {
        return Ok(false);
    }
    let git_dir = PathBuf::from(git_stdout(&output)).canonicalize().ok();
    let expected_git_dir = root.join(".git").canonicalize().ok();
    Ok(git_dir == expected_git_dir)
}

fn read_sidecar_repo_status(repo: &ResolvedSidecarRepo) -> ExoResult<SidecarRepoStatusOutput> {
    let files = read_status_files(&repo.sidecar_root)?;
    let project_files = owned_sidecar_status_files(repo)?;
    let foreign_checkpoint_debt = foreign_checkpoint_debt(repo, &files)?;
    let remote = first_remote(&repo.sidecar_root)?;
    let branch = current_branch(&repo.sidecar_root)?;
    let ownership = read_sidecar_write_ownership_status(repo)?;
    let upstream_relation = branch
        .as_deref()
        .map(|branch| read_upstream_relation(&repo.sidecar_root, branch))
        .transpose()?
        .flatten();
    let (ahead, behind) = upstream_relation
        .as_ref()
        .map(|relation| (relation.ahead, relation.behind))
        .unwrap_or((None, None));
    let sync_issue = sidecar_sync_issue(
        files.is_empty(),
        remote.as_deref(),
        upstream_relation.as_ref(),
    );
    let issue = sidecar_repo_issue(sync_issue, &foreign_checkpoint_debt);
    let syncable = sidecar_repo_is_syncable(remote.as_deref(), branch.as_deref(), issue.as_ref());
    let next_actions = sidecar_repo_status_actions(issue.as_ref(), &foreign_checkpoint_debt);
    Ok(SidecarRepoStatusOutput {
        kind: "sidecar.repo.status",
        ok: issue
            .as_ref()
            .is_none_or(|issue| issue.kind != "unrelated_history"),
        sidecar_root: repo.sidecar_root.clone(),
        branch,
        clean: project_files.is_empty(),
        repo_clean: files.is_empty(),
        syncable,
        has_remote: remote.is_some(),
        remote,
        ahead,
        behind,
        issue_kind: issue.as_ref().map(|issue| issue.kind),
        issue: issue.map(|issue| issue.message),
        ownership,
        files,
        project_files,
        foreign_checkpoint_debt,
        next_actions,
    })
}

fn read_sidecar_repo_sync_status(repo: &ResolvedSidecarRepo) -> ExoResult<SidecarRepoSyncStatus> {
    if !is_independent_git_repo(&repo.sidecar_root)? {
        return Ok(SidecarRepoSyncStatus {
            kind: "sidecar.repo.sync_status",
            ok: false,
            sidecar_root: repo.sidecar_root.clone(),
            branch: None,
            clean: false,
            repo_clean: false,
            syncable: false,
            has_remote: false,
            remote: None,
            ahead: None,
            behind: None,
            issue_kind: Some("invalid_repo"),
            issue: Some("sidecar root is not an independent git repository".to_string()),
            project_files: Vec::new(),
            foreign_checkpoint_debt: Vec::new(),
        });
    }
    let files = read_status_files(&repo.sidecar_root)?;
    let project_files = owned_sidecar_status_files(repo)?;
    let foreign_checkpoint_debt = foreign_checkpoint_debt(repo, &files)?;
    let remote = first_remote(&repo.sidecar_root)?;
    let branch = current_branch(&repo.sidecar_root)?;
    let upstream_relation = branch
        .as_deref()
        .map(|branch| read_upstream_relation(&repo.sidecar_root, branch))
        .transpose()?
        .flatten();
    let (ahead, behind) = upstream_relation
        .as_ref()
        .map(|relation| (relation.ahead, relation.behind))
        .unwrap_or((None, None));
    let sync_issue = sidecar_sync_issue(
        files.is_empty(),
        remote.as_deref(),
        upstream_relation.as_ref(),
    );
    let issue = sidecar_repo_issue(sync_issue, &foreign_checkpoint_debt);
    let syncable = sidecar_repo_is_syncable(remote.as_deref(), branch.as_deref(), issue.as_ref());
    Ok(SidecarRepoSyncStatus {
        kind: "sidecar.repo.sync_status",
        ok: issue.is_none(),
        sidecar_root: repo.sidecar_root.clone(),
        branch,
        clean: project_files.is_empty(),
        repo_clean: files.is_empty(),
        syncable,
        has_remote: remote.is_some(),
        remote,
        ahead,
        behind,
        issue_kind: issue.as_ref().map(|issue| issue.kind),
        issue: issue.map(|issue| issue.message),
        project_files,
        foreign_checkpoint_debt,
    })
}

fn sidecar_checkpoint_debt_issue(
    foreign_checkpoint_debt: &[SidecarForeignCheckpointDebt],
) -> Option<SidecarRepoIssue> {
    (!foreign_checkpoint_debt.is_empty()).then(|| {
        SidecarRepoIssue::new(
            "foreign_checkpoint_debt",
            "sidecar repo has foreign or cross-project checkpoint debt; resolve those project changes before syncing",
        )
    })
}

fn sidecar_repo_issue(
    sync_issue: Option<SidecarRepoIssue>,
    foreign_checkpoint_debt: &[SidecarForeignCheckpointDebt],
) -> Option<SidecarRepoIssue> {
    if sync_issue
        .as_ref()
        .is_some_and(|issue| issue.kind == "unrelated_history")
    {
        return sync_issue;
    }

    sidecar_checkpoint_debt_issue(foreign_checkpoint_debt).or(sync_issue)
}

fn sidecar_repo_status_actions(
    issue: Option<&SidecarRepoIssue>,
    foreign_checkpoint_debt: &[SidecarForeignCheckpointDebt],
) -> Vec<SuggestedAction> {
    if issue.is_some_and(|issue| issue.kind == "unrelated_history") {
        return issue
            .map(|issue| sidecar_repo_issue_actions(issue.kind))
            .unwrap_or_default();
    }
    if !foreign_checkpoint_debt.is_empty() {
        return sidecar_checkpoint_debt_actions(foreign_checkpoint_debt);
    }
    issue
        .map(|issue| sidecar_repo_issue_actions(issue.kind))
        .unwrap_or_default()
}

fn sidecar_checkpoint_debt_actions(
    foreign_checkpoint_debt: &[SidecarForeignCheckpointDebt],
) -> Vec<SuggestedAction> {
    foreign_checkpoint_debt
        .iter()
        .flat_map(|debt| debt.next_actions.iter().cloned())
        .collect()
}

fn sidecar_sync_issue(
    clean: bool,
    remote: Option<&str>,
    upstream_relation: Option<&SidecarUpstreamRelation>,
) -> Option<SidecarRepoIssue> {
    if upstream_relation.is_some_and(|relation| !relation.has_merge_base) {
        return Some(SidecarRepoIssue::new(
            "unrelated_history",
            "sidecar repo local branch and upstream have unrelated history; Exo cannot safely sync this sidecar repo automatically",
        ));
    }
    if !clean {
        return Some(SidecarRepoIssue::new(
            "dirty",
            "sidecar repo has uncommitted changes",
        ));
    }
    if remote.is_none() {
        return Some(SidecarRepoIssue::new(
            "no_remote",
            "sidecar repo has no remote",
        ));
    }
    let Some(upstream_relation) = upstream_relation else {
        return Some(SidecarRepoIssue::new(
            "no_upstream",
            "sidecar repo has commits that have not been pushed",
        ));
    };
    if upstream_relation.ahead.unwrap_or(0) > 0 {
        return Some(SidecarRepoIssue::new(
            "ahead",
            "sidecar repo has commits that have not been pushed",
        ));
    }
    if upstream_relation.behind.unwrap_or(0) > 0 {
        return Some(SidecarRepoIssue::new(
            "behind",
            "sidecar repo is behind its upstream",
        ));
    }
    None
}

fn foreign_checkpoint_debt(
    repo: &ResolvedSidecarRepo,
    files: &[SidecarRepoFileStatus],
) -> ExoResult<Vec<SidecarForeignCheckpointDebt>> {
    let current_project_path = owned_sidecar_project_path(repo)?;
    let known_project_keys = known_sidecar_project_keys(repo)?;
    let mut grouped: BTreeMap<String, Vec<SidecarRepoFileStatus>> = BTreeMap::new();
    let mut cross_project_files = Vec::new();
    for file in files {
        let path_parts = sidecar_status_path_parts(&file.path).collect::<Vec<_>>();
        let project_keys_in_status_entry = path_parts
            .iter()
            .map(|path| {
                if sidecar_status_file_is_untracked_project_runtime(file, &known_project_keys)
                    && sidecar_path_is_project_runtime(path, &known_project_keys)
                {
                    None
                } else {
                    sidecar_project_key_from_status_path(path, &known_project_keys)
                }
            })
            .collect::<Vec<_>>();
        let projects_in_status_entry = project_keys_in_status_entry
            .iter()
            .filter_map(|project| project.clone())
            .collect::<BTreeSet<_>>();
        let status_entry_has_non_project_path =
            project_keys_in_status_entry.iter().any(Option::is_none);
        if path_parts.len() > 1
            && !projects_in_status_entry.is_empty()
            && (projects_in_status_entry.len() > 1 || status_entry_has_non_project_path)
        {
            cross_project_files.push(file.clone());
            continue;
        }

        let mut projects_for_file = BTreeSet::new();
        for path in path_parts {
            if sidecar_status_file_is_untracked_project_runtime(file, &known_project_keys)
                && sidecar_path_is_project_runtime(path, &known_project_keys)
            {
                continue;
            }
            if sidecar_path_matches(path, &current_project_path) || path == ".gitignore" {
                continue;
            }
            let Some(project) = sidecar_project_key_from_status_path(path, &known_project_keys)
            else {
                continue;
            };
            projects_for_file.insert(project);
        }
        for project in projects_for_file {
            grouped.entry(project).or_default().push(file.clone());
        }
    }

    let mut debt = grouped
        .into_iter()
        .map(|(project, files)| {
            let command = ExoCommandReference::new(&["sidecar", "checkpoint"])
                .option("project", project.clone());
            SidecarForeignCheckpointDebt {
                project: project.clone(),
                checkpointable: true,
                issue_kind: None,
                files,
                next_actions: vec![SuggestedAction::exo(
                    format!("Checkpoint sidecar project {project}"),
                    command,
                    "Complete the local checkpoint for this sidecar project.",
                    WorkIntent::Execute,
                    Some(0.9),
                )],
            }
        })
        .collect::<Vec<_>>();
    if !cross_project_files.is_empty() {
        debt.push(SidecarForeignCheckpointDebt {
            project: "cross-project".to_string(),
            checkpointable: false,
            issue_kind: Some("cross_project_move"),
            files: cross_project_files,
            next_actions: vec![SuggestedAction::exo(
                "Inspect cross-project sidecar move",
                ExoCommandReference::new(&["sidecar", "repo", "status"]),
                "Review the cross-project sidecar move before choosing a repair.",
                WorkIntent::Orient,
                Some(0.9),
            )],
        });
    }
    Ok(debt)
}

#[derive(Deserialize)]
struct SidecarProjectManifest {
    sidecar: Option<SidecarProjectManifestSection>,
}

#[derive(Deserialize)]
struct SidecarProjectManifestSection {
    key: Option<String>,
}

fn known_sidecar_project_keys(repo: &ResolvedSidecarRepo) -> ExoResult<Vec<String>> {
    let mut keys = BTreeSet::new();
    if let Some(key) = repo.project.sidecar_key.as_ref() {
        keys.insert(key.clone());
    }

    let projects_dir = repo.sidecar_root.join("projects");
    collect_known_sidecar_project_keys(&projects_dir, &projects_dir, &mut keys)?;
    collect_tracked_sidecar_project_keys(&repo.sidecar_root, &mut keys)?;

    let mut keys = keys.into_iter().collect::<Vec<_>>();
    keys.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
    Ok(keys)
}

fn collect_tracked_sidecar_project_keys(
    sidecar_root: &Path,
    keys: &mut BTreeSet<String>,
) -> ExoResult<()> {
    let output = run_git(sidecar_root, &["ls-files", "projects"])?;
    if !output.status.success() {
        return Ok(());
    }

    for path in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(key) = sidecar_project_key_from_manifest_status_path(path) {
            keys.insert(key);
        }
    }

    Ok(())
}

fn collect_known_sidecar_project_keys(
    projects_dir: &Path,
    dir: &Path,
    keys: &mut BTreeSet<String>,
) -> ExoResult<()> {
    if !dir.exists() {
        return Ok(());
    }

    let manifest_path = dir.join("sidecar.toml");
    if manifest_path.exists()
        && let Some(key) = sidecar_project_key_from_manifest_or_path(projects_dir, dir)?
    {
        keys.insert(key);
    }

    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("Failed to read sidecar project directory {}", dir.display()))?
    {
        let entry = entry.with_context(|| {
            format!("Failed to read sidecar project entry in {}", dir.display())
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_known_sidecar_project_keys(projects_dir, &path, keys)?;
        }
    }

    Ok(())
}

fn sidecar_project_key_from_manifest_or_path(
    projects_dir: &Path,
    project_dir: &Path,
) -> ExoResult<Option<String>> {
    let manifest_path = project_dir.join("sidecar.toml");
    let manifest = std::fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "Failed to read sidecar manifest {}",
            manifest_path.display()
        )
    })?;
    let manifest_key = toml::from_str::<SidecarProjectManifest>(&manifest)
        .with_context(|| {
            format!(
                "Failed to parse sidecar manifest {}",
                manifest_path.display()
            )
        })?
        .sidecar
        .and_then(|sidecar| sidecar.key);
    if let Some(key) = manifest_key {
        return Ok(Some(key));
    }

    let key = project_dir
        .strip_prefix(projects_dir)
        .ok()
        .and_then(|path| path.to_str())
        .filter(|key| !key.is_empty())
        .map(|key| key.replace(std::path::MAIN_SEPARATOR, "/"));
    Ok(key)
}

fn sidecar_project_key_from_manifest_status_path(path: &str) -> Option<String> {
    let rest = path.strip_prefix("projects/")?;
    let key = rest.strip_suffix("/sidecar.toml")?;
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

fn sidecar_project_key_from_status_path(path: &str, known_keys: &[String]) -> Option<String> {
    let rest = path.strip_prefix("projects/")?;
    for key in known_keys {
        if rest == key
            || rest
                .strip_prefix(key)
                .is_some_and(|rest| rest.starts_with('/'))
        {
            return Some(key.clone());
        }
    }
    let key = rest.split('/').next()?;
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

fn sidecar_project_has_status_debt(sidecar_root: &Path, key: &str) -> ExoResult<bool> {
    let project_path = format!("projects/{key}");
    Ok(read_status_files(sidecar_root)?
        .iter()
        .any(|file| sidecar_status_path_mentions(&file.path, &project_path)))
}

fn sidecar_status_path_is_project_runtime(path: &str, known_keys: &[String]) -> bool {
    sidecar_status_path_parts(path).any(|path| sidecar_path_is_project_runtime(path, known_keys))
}

fn sidecar_status_file_is_untracked_project_runtime(
    file: &SidecarRepoFileStatus,
    known_keys: &[String],
) -> bool {
    file.status == "??" && sidecar_status_path_is_project_runtime(&file.path, known_keys)
}

fn sidecar_path_is_project_runtime(path: &str, known_keys: &[String]) -> bool {
    let Some(rest) = path.strip_prefix("projects/") else {
        return false;
    };
    let Some(key) = sidecar_project_key_from_status_path(path, known_keys) else {
        return false;
    };
    let Some(project_path) = rest
        .strip_prefix(&key)
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        return false;
    };
    project_path.starts_with("cache/") || project_path.starts_with("runtime/")
}

fn sidecar_repo_is_syncable(
    remote: Option<&str>,
    branch: Option<&str>,
    issue: Option<&SidecarRepoIssue>,
) -> bool {
    remote.is_some()
        && branch.is_some()
        && issue.is_none_or(|issue| {
            !matches!(
                issue.kind,
                "invalid_repo"
                    | "no_remote"
                    | "detached_head"
                    | "unrelated_history"
                    | "foreign_checkpoint_debt"
            )
        })
}

fn sidecar_repo_sync_actions(status: &SidecarRepoSyncStatus) -> Vec<SuggestedAction> {
    if status.issue_kind == Some("unrelated_history") {
        return sidecar_repo_issue_actions("unrelated_history");
    }
    if !status.foreign_checkpoint_debt.is_empty() {
        return sidecar_checkpoint_debt_actions(&status.foreign_checkpoint_debt);
    }
    status
        .issue_kind
        .map(sidecar_repo_issue_actions)
        .unwrap_or_default()
}

fn sidecar_repo_issue_actions(issue_kind: &'static str) -> Vec<SuggestedAction> {
    let (label, command, rationale, confidence) = match issue_kind {
        "no_remote" => (
            "Add sidecar remote",
            "exo sidecar repo remote --url <url>",
            "Configure the sidecar remote before publishing portable state.",
            1.0,
        ),
        "behind" => (
            "Sync sidecar repo",
            "exo sidecar repo sync",
            "Merge portable sidecar state semantically before pushing local state.",
            0.9,
        ),
        "ahead" | "no_upstream" => (
            "Push sidecar repo",
            "exo sidecar repo push",
            "Publish committed sidecar state to the configured remote.",
            0.9,
        ),
        "unrelated_history" => (
            "Inspect sidecar repo recovery",
            "exo sidecar repo status",
            "The sidecar repo has unrelated local and upstream histories; inspect Exo's structured recovery state before changing it.",
            0.9,
        ),
        _ => (
            "Inspect sidecar repo",
            "exo sidecar repo status",
            "Inspect the sidecar repository before retrying sidecar sync.",
            0.8,
        ),
    };
    vec![SuggestedAction {
        label: label.to_string(),
        command: command.to_string(),
        rationale: rationale.to_string(),
        intent: WorkIntent::Execute,
        confidence: Some(confidence),
    }]
}

fn auto_persist_resolved_sidecar_repo_result(
    repo: &ResolvedSidecarRepo,
) -> ExoResult<SidecarAutoPersistReport> {
    let checkpoint = commit_local_sidecar_checkpoint(repo, "Auto-persist Exosuit sidecar state")?;

    let mut report = SidecarAutoPersistReport {
        kind: "sidecar.auto_persist",
        ok: true,
        auto_commit: repo.project.sidecar_auto_commit,
        auto_push: repo.project.sidecar_auto_push.as_str(),
        committed: checkpoint.committed,
        commit: checkpoint.commit,
        files_changed: checkpoint.files_changed,
        pushed: false,
        remote: None,
        branch: checkpoint.branch.clone(),
        issue: None,
    };

    if should_auto_push(repo.project.sidecar_auto_push, &repo.sidecar_root)? {
        let foreign_checkpoint_debt =
            foreign_checkpoint_debt(repo, &read_status_files(&repo.sidecar_root)?)?;
        if !foreign_checkpoint_debt.is_empty() {
            report.ok = false;
            report.issue = Some(
                "sidecar repo has foreign or cross-project checkpoint debt; resolve those project changes before auto-push"
                    .to_string(),
            );
            return Ok(report);
        }
        let Some(branch) = checkpoint.branch else {
            report.ok = false;
            report.issue = Some("sidecar auto-push requires a named branch".to_string());
            return Ok(report);
        };
        let Some(remote) = first_remote(&repo.sidecar_root)? else {
            report.ok = false;
            report.issue = Some("sidecar auto-push requires a configured remote".to_string());
            return Ok(report);
        };
        if let Err(error) = fetch_remote(&repo.sidecar_root, &remote) {
            report.ok = false;
            report.remote = Some(remote);
            report.branch = Some(branch);
            report.issue = Some(error.to_string());
            return Ok(report);
        }

        match semantic_merge_upstream_if_needed(repo, &branch) {
            Ok(merge) if !merge.conflicts.is_empty() => {
                report.ok = false;
                report.remote = Some(remote);
                report.branch = Some(branch);
                report.issue = Some(format!(
                    "sidecar SQL projection has conflicting row changes ({} conflict(s))",
                    merge.conflicts.len()
                ));
                return Ok(report);
            }
            Ok(merge) => {
                if let Some(merge_commit) = merge.commit {
                    report.committed = true;
                    report.commit = Some(merge_commit);
                }
            }
            Err(error) => {
                report.ok = false;
                report.remote = Some(remote);
                report.branch = Some(branch);
                report.issue = Some(error.to_string());
                return Ok(report);
            }
        }

        match push_sidecar_branch_with_recovery(repo, &remote, &branch) {
            Ok(push) => {
                report.pushed = push.pushed;
                report.remote = Some(remote.clone());
                report.branch = Some(branch);
            }
            Err(error) => {
                report.ok = false;
                report.remote = Some(remote);
                report.branch = Some(branch);
                report.issue = Some(error.to_string());
            }
        }
    }

    Ok(report)
}

fn sync_resolved_sidecar_repo(repo: &ResolvedSidecarRepo) -> ExoResult<SidecarRepoSyncOutput> {
    ensure_sidecar_write_ownership(repo)?;
    let foreign_checkpoint_debt =
        foreign_checkpoint_debt(repo, &read_status_files(&repo.sidecar_root)?)?;
    if !foreign_checkpoint_debt.is_empty() {
        return Ok(SidecarRepoSyncOutput {
            kind: "sidecar.repo.sync",
            ok: false,
            sidecar_root: repo.sidecar_root.clone(),
            branch: current_branch(&repo.sidecar_root)?,
            committed: false,
            commit: None,
            merged: false,
            pushed: false,
            remote: first_remote(&repo.sidecar_root)?,
            issue_kind: Some("foreign_checkpoint_debt"),
            issue: Some(
                "sidecar repo has foreign or cross-project checkpoint debt; resolve those project changes before syncing"
                    .to_string(),
            ),
            conflicts: Vec::new(),
        });
    }
    crate::context::write_sql_dump_with_project_result(
        &repo.project_workspace_root(),
        Some(&repo.project),
    )?;
    ensure_sidecar_runtime_gitignore(&repo.sidecar_root)?;
    ensure_sidecar_runtime_paths_untracked(repo)?;

    let (committed, mut commit, _) =
        commit_sidecar_changes(repo, "Auto-persist Exosuit sidecar state")?;
    let branch = current_branch(&repo.sidecar_root)?;
    let Some(branch_name) = branch.as_deref() else {
        return Ok(SidecarRepoSyncOutput {
            kind: "sidecar.repo.sync",
            ok: false,
            sidecar_root: repo.sidecar_root.clone(),
            branch,
            committed,
            commit,
            merged: false,
            pushed: false,
            remote: None,
            issue_kind: Some("detached_head"),
            issue: Some("sidecar sync requires a named branch".to_string()),
            conflicts: Vec::new(),
        });
    };
    let Some(remote) = first_remote(&repo.sidecar_root)? else {
        return Ok(SidecarRepoSyncOutput {
            kind: "sidecar.repo.sync",
            ok: false,
            sidecar_root: repo.sidecar_root.clone(),
            branch,
            committed,
            commit,
            merged: false,
            pushed: false,
            remote: None,
            issue_kind: Some("no_remote"),
            issue: Some("sidecar sync requires a configured remote".to_string()),
            conflicts: Vec::new(),
        });
    };

    fetch_remote(&repo.sidecar_root, &remote)?;
    if let Some(relation) = read_upstream_relation(&repo.sidecar_root, branch_name)?
        && !relation.has_merge_base
    {
        return Ok(SidecarRepoSyncOutput {
            kind: "sidecar.repo.sync",
            ok: false,
            sidecar_root: repo.sidecar_root.clone(),
            branch,
            committed,
            commit,
            merged: false,
            pushed: false,
            remote: Some(remote),
            issue_kind: Some("unrelated_history"),
            issue: Some(
                "sidecar repo local branch and upstream have unrelated history; Exo cannot safely sync this sidecar repo automatically"
                    .to_string(),
            ),
            conflicts: Vec::new(),
        });
    }
    let merge = semantic_merge_upstream_if_needed(repo, branch_name)?;
    if !merge.conflicts.is_empty() {
        return Ok(SidecarRepoSyncOutput {
            kind: "sidecar.repo.sync",
            ok: false,
            sidecar_root: repo.sidecar_root.clone(),
            branch,
            committed,
            commit,
            merged: false,
            pushed: false,
            remote: Some(remote),
            issue_kind: Some("sql_conflict"),
            issue: Some("sidecar SQL projection has conflicting row changes".to_string()),
            conflicts: merge.conflicts,
        });
    }
    if let Some(merge_commit) = merge.commit {
        commit = Some(merge_commit);
    }

    let push = push_sidecar_branch_with_recovery(repo, &remote, branch_name)?;

    Ok(SidecarRepoSyncOutput {
        kind: "sidecar.repo.sync",
        ok: true,
        sidecar_root: repo.sidecar_root.clone(),
        branch,
        committed: committed || merge.merged,
        commit,
        merged: merge.merged,
        pushed: push.pushed,
        remote: Some(remote),
        issue_kind: None,
        issue: None,
        conflicts: Vec::new(),
    })
}

fn commit_sidecar_changes(
    repo: &ResolvedSidecarRepo,
    message: &str,
) -> ExoResult<(bool, Option<String>, usize)> {
    ensure_sidecar_write_ownership(repo)?;
    crate::project::ensure_sidecar_git_identity(&repo.sidecar_root)?;
    ensure_no_unowned_staged_sidecar_paths(repo)?;
    let files_changed = owned_sidecar_status_files(repo)?.len();
    if files_changed == 0 {
        return Ok((false, None, 0));
    }

    stage_owned_sidecar_paths(repo)?;
    ensure_no_unowned_staged_sidecar_paths(repo)?;
    run_git_checked(&repo.sidecar_root, &["commit", "-m", message], "git commit")?;
    let commit = git_stdout(&run_git_checked(
        &repo.sidecar_root,
        &["rev-parse", "--short", "HEAD"],
        "git rev-parse --short HEAD",
    )?);

    Ok((true, Some(commit), files_changed))
}

fn owned_sidecar_status_files(repo: &ResolvedSidecarRepo) -> ExoResult<Vec<SidecarRepoFileStatus>> {
    let project_path = owned_sidecar_project_path(repo)?;
    let known_project_keys = known_sidecar_project_keys(repo)?;
    Ok(read_status_files(&repo.sidecar_root)?
        .into_iter()
        .filter(|file| {
            !sidecar_status_file_is_untracked_project_runtime(file, &known_project_keys)
                && sidecar_status_path_is_owned(&file.path, &project_path)
        })
        .collect())
}

fn unowned_sidecar_status_files(
    repo: &ResolvedSidecarRepo,
) -> ExoResult<Vec<SidecarRepoFileStatus>> {
    let project_path = owned_sidecar_project_path(repo)?;
    Ok(read_status_files(&repo.sidecar_root)?
        .into_iter()
        .filter(|file| !sidecar_status_path_is_owned(&file.path, &project_path))
        .collect())
}

fn ensure_no_unowned_staged_sidecar_paths(repo: &ResolvedSidecarRepo) -> ExoResult<()> {
    let paths = unowned_staged_sidecar_paths(repo)?;
    if paths.is_empty() {
        return Ok(());
    }

    Err(anyhow::Error::new(ExoFailure::new(
        ErrorCode::PreconditionFailed,
        format!(
            "sidecar repo has staged changes outside the current project subtree: {}. Unstage those paths before committing sidecar state.",
            paths.join(", ")
        ),
        ExoFailure::orienting_steering(vec![SuggestedAction {
            label: "Inspect sidecar repo".to_string(),
            command: "exo sidecar repo status".to_string(),
            rationale: "Review staged sidecar paths before committing portable state.".to_string(),
            intent: WorkIntent::Orient,
            confidence: Some(1.0),
        }]),
    )))
}

fn unowned_staged_sidecar_paths(repo: &ResolvedSidecarRepo) -> ExoResult<Vec<String>> {
    let project_path = owned_sidecar_project_path(repo)?;
    let output = run_git_checked(
        &repo.sidecar_root,
        &["diff", "--cached", "--name-status", "-z"],
        "git diff --cached --name-status",
    )?;
    Ok(staged_diff_paths(&output.stdout)
        .into_iter()
        .filter(|path| !sidecar_single_path_is_owned(path, &project_path))
        .collect())
}

fn stage_owned_sidecar_paths(repo: &ResolvedSidecarRepo) -> ExoResult<()> {
    let project_path = owned_sidecar_project_path(repo)?;
    let known_project_keys = known_sidecar_project_keys(repo)?;
    let status_files = owned_sidecar_status_files(repo)?;
    let mut pathspecs = BTreeSet::new();
    for file in &status_files {
        for path in sidecar_status_path_parts(&file.path) {
            if sidecar_path_is_project_runtime(path, &known_project_keys) {
                continue;
            }
            if sidecar_path_matches(path, &project_path) || path == ".gitignore" {
                pathspecs.insert(path.to_string());
            }
        }
    }
    if pathspecs.is_empty() {
        return Ok(());
    }

    let mut args = vec!["add".to_string(), "-A".to_string(), "--".to_string()];
    args.extend(pathspecs);
    let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    run_git_checked(
        &repo.sidecar_root,
        &arg_refs,
        "git add -A -- <owned sidecar paths>",
    )?;
    Ok(())
}

fn owned_sidecar_project_path(repo: &ResolvedSidecarRepo) -> ExoResult<String> {
    let key = repo.project.sidecar_key.as_deref().ok_or_else(|| {
        anyhow::anyhow!("sidecar repo is missing the current project sidecar key")
    })?;
    Ok(format!("projects/{key}"))
}

fn sidecar_status_path_is_owned(path: &str, project_path: &str) -> bool {
    sidecar_status_path_parts(path).all(|part| sidecar_single_path_is_owned(part, project_path))
}

fn sidecar_status_path_mentions(path: &str, owned_path: &str) -> bool {
    sidecar_status_path_parts(path).any(|part| sidecar_path_matches(part, owned_path))
}

fn sidecar_status_path_parts(path: &str) -> impl Iterator<Item = &str> {
    path.split(" -> ")
}

fn sidecar_single_path_is_owned(path: &str, project_path: &str) -> bool {
    sidecar_path_matches(path, project_path) || sidecar_path_matches(path, ".gitignore")
}

fn sidecar_path_matches(path: &str, owned_path: &str) -> bool {
    path == owned_path
        || path
            .strip_prefix(owned_path)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn staged_diff_paths(stdout: &[u8]) -> Vec<String> {
    let mut fields = stdout
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    let mut paths = Vec::new();
    while let Some(status) = fields.next() {
        let status = String::from_utf8_lossy(status);
        let path_count = if status.starts_with('R') || status.starts_with('C') {
            2
        } else {
            1
        };
        for _ in 0..path_count {
            let Some(path) = fields.next() else {
                break;
            };
            paths.push(String::from_utf8_lossy(path).to_string());
        }
    }
    paths
}

fn restore_upstream_foreign_projects_for_merge(
    repo: &ResolvedSidecarRepo,
    upstream: &str,
) -> ExoResult<()> {
    let current_key = repo.project.sidecar_key.as_deref().ok_or_else(|| {
        anyhow::anyhow!("sidecar repo is missing the current project sidecar key")
    })?;
    let projects_spec = format!("{upstream}:projects");
    let exists = run_git(&repo.sidecar_root, &["cat-file", "-e", &projects_spec])?;
    if !exists.status.success() {
        return Ok(());
    }

    let output = run_git_checked(
        &repo.sidecar_root,
        &["ls-tree", "-z", "--name-only", &projects_spec],
        "git ls-tree upstream projects",
    )?;
    for name in output.stdout.split(|byte| *byte == 0) {
        if name.is_empty() {
            continue;
        }
        let name = String::from_utf8_lossy(name);
        if name == current_key {
            continue;
        }
        let path = format!("projects/{name}");
        run_git_checked(
            &repo.sidecar_root,
            &["checkout", upstream, "--", &path],
            "git checkout upstream foreign sidecar project",
        )?;
    }

    Ok(())
}

#[derive(Debug, Default)]
struct SemanticMergeReport {
    merged: bool,
    commit: Option<String>,
    conflicts: Vec<SqlProjectionConflict>,
}

#[derive(Debug, Clone, Serialize)]
struct SqlProjectionConflict {
    file: String,
    table: String,
    row_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SqlRowKey {
    file: String,
    table: String,
    row_id: String,
}

#[derive(Debug, Default)]
struct SqlProjectionFile {
    rows: BTreeMap<SqlRowKey, String>,
}

fn semantic_merge_upstream_if_needed(
    repo: &ResolvedSidecarRepo,
    branch: &str,
) -> ExoResult<SemanticMergeReport> {
    let Some(upstream) = upstream_ref_for_branch(&repo.sidecar_root, branch, None)? else {
        return Ok(SemanticMergeReport::default());
    };
    let Some(relation) = read_upstream_relation(&repo.sidecar_root, branch)? else {
        return Ok(SemanticMergeReport::default());
    };
    if relation.behind.unwrap_or(0) == 0 {
        return Ok(SemanticMergeReport::default());
    }
    if !relation.has_merge_base {
        anyhow::bail!(
            "sidecar repo local branch and upstream have unrelated history; Exo cannot safely sync this sidecar repo automatically"
        );
    }

    let base = git_stdout(&run_git_checked(
        &repo.sidecar_root,
        &["merge-base", "HEAD", &upstream],
        "git merge-base",
    )?);
    let merge = merge_sql_projection_at_revisions(repo, &base, "HEAD", &upstream)?;
    if !merge.conflicts.is_empty() {
        return Ok(SemanticMergeReport {
            conflicts: merge.conflicts,
            ..SemanticMergeReport::default()
        });
    }

    ensure_no_unowned_staged_sidecar_paths(repo)?;
    run_git_checked(
        &repo.sidecar_root,
        &["merge", "-s", "ours", "--no-commit", &upstream],
        "git merge -s ours --no-commit",
    )?;
    restore_upstream_foreign_projects_for_merge(repo, &upstream)?;
    for (relative_path, content) in merge.files {
        let path = repo.sidecar_root.join(relative_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write merged SQL projection {}", path.display()))?;
    }
    if let Some(projection_dir) = repo.project.sidecar_projection_dir() {
        crate::context::import_sql_dumps(&projection_dir, &repo.project.db_path())?;
        crate::context::write_sql_dump_with_project_result(
            &repo.project_workspace_root(),
            Some(&repo.project),
        )?;
    }
    ensure_sidecar_runtime_paths_untracked(repo)?;
    stage_owned_sidecar_paths(repo)?;
    run_git_checked(
        &repo.sidecar_root,
        &["commit", "-m", "Auto-merge Exosuit sidecar state"],
        "git commit",
    )?;
    let commit = git_stdout(&run_git_checked(
        &repo.sidecar_root,
        &["rev-parse", "--short", "HEAD"],
        "git rev-parse --short HEAD",
    )?);

    Ok(SemanticMergeReport {
        merged: true,
        commit: Some(commit),
        conflicts: Vec::new(),
    })
}

struct SqlProjectionMerge {
    files: BTreeMap<PathBuf, String>,
    conflicts: Vec<SqlProjectionConflict>,
}

fn merge_sql_projection_at_revisions(
    repo: &ResolvedSidecarRepo,
    base_rev: &str,
    local_rev: &str,
    remote_rev: &str,
) -> ExoResult<SqlProjectionMerge> {
    let mut files = BTreeMap::new();
    let mut conflicts = Vec::new();

    for relative_path in projection_relative_paths(repo)? {
        let base = parse_sql_projection_file(
            &relative_path,
            &git_show_or_empty(&repo.sidecar_root, base_rev, &relative_path)?,
        );
        let local = parse_sql_projection_file(
            &relative_path,
            &git_show_or_empty(&repo.sidecar_root, local_rev, &relative_path)?,
        );
        let remote = parse_sql_projection_file(
            &relative_path,
            &git_show_or_empty(&repo.sidecar_root, remote_rev, &relative_path)?,
        );
        let merged = merge_sql_projection_file(&base, &local, &remote);
        conflicts.extend(merged.conflicts);
        files.insert(relative_path, render_sql_projection_rows(merged.rows));
    }

    Ok(SqlProjectionMerge { files, conflicts })
}

struct SqlProjectionFileMerge {
    rows: BTreeMap<SqlRowKey, String>,
    conflicts: Vec<SqlProjectionConflict>,
}

fn merge_sql_projection_file(
    base: &SqlProjectionFile,
    local: &SqlProjectionFile,
    remote: &SqlProjectionFile,
) -> SqlProjectionFileMerge {
    let mut keys = BTreeSet::new();
    keys.extend(base.rows.keys().cloned());
    keys.extend(local.rows.keys().cloned());
    keys.extend(remote.rows.keys().cloned());

    let mut rows = BTreeMap::new();
    let mut conflicts = Vec::new();

    for key in keys {
        let base_row = base.rows.get(&key);
        let local_row = local.rows.get(&key);
        let remote_row = remote.rows.get(&key);

        let chosen = match (base_row, local_row, remote_row) {
            (_, Some(local), Some(remote)) if local == remote => Some(local.clone()),
            (Some(base), Some(local), Some(remote)) if local == base => Some(remote.clone()),
            (Some(base), Some(local), Some(remote)) if remote == base => Some(local.clone()),
            (None, Some(local), None) => Some(local.clone()),
            (None, None, Some(remote)) => Some(remote.clone()),
            (Some(base), None, Some(remote)) if remote == base => None,
            (Some(base), Some(local), None) if local == base => None,
            (None, None, None) => None,
            _ => {
                conflicts.push(SqlProjectionConflict {
                    file: key.file.clone(),
                    table: key.table.clone(),
                    row_id: key.row_id.clone(),
                });
                local_row.cloned().or_else(|| remote_row.cloned())
            }
        };

        if let Some(row) = chosen {
            rows.insert(key, row);
        }
    }

    SqlProjectionFileMerge { rows, conflicts }
}

fn projection_relative_paths(repo: &ResolvedSidecarRepo) -> ExoResult<Vec<PathBuf>> {
    let projection_dir = repo
        .project
        .sidecar_projection_dir()
        .ok_or_else(|| anyhow::anyhow!("sidecar project is missing a SQL projection directory"))?;
    let relative_dir = projection_dir
        .strip_prefix(&repo.sidecar_root)
        .with_context(|| {
            format!(
                "sidecar projection {} is not under sidecar root {}",
                projection_dir.display(),
                repo.sidecar_root.display()
            )
        })?;

    Ok(exosuit_storage::TABLE_ORDER
        .iter()
        .map(|(file_stem, _)| relative_dir.join(format!("{file_stem}.sql")))
        .collect())
}

fn parse_sql_projection_file(relative_path: &Path, content: &str) -> SqlProjectionFile {
    let file = git_relative_path(relative_path);
    let mut rows = BTreeMap::new();
    for line in content.lines() {
        let line = line.trim();
        if !line.starts_with("INSERT INTO ") {
            continue;
        }
        let key = sql_row_key(&file, line);
        rows.insert(key, line.to_string());
    }
    SqlProjectionFile { rows }
}

fn render_sql_projection_rows(rows: BTreeMap<SqlRowKey, String>) -> String {
    let mut rendered = String::from("-- Auto-generated by exo. Regenerate: exo status\n");
    for row in rows.into_values() {
        rendered.push_str(&row);
        rendered.push('\n');
    }
    rendered
}

fn sql_row_key(file: &str, line: &str) -> SqlRowKey {
    let Some(rest) = line.strip_prefix("INSERT INTO ") else {
        return fallback_sql_row_key(file, line);
    };
    let Some((table, rest)) = rest.split_once('(') else {
        return fallback_sql_row_key(file, line);
    };
    let Some((columns, rest)) = rest.split_once(") VALUES(") else {
        return fallback_sql_row_key(file, line);
    };
    let Some(values) = rest.strip_suffix(");") else {
        return fallback_sql_row_key(file, line);
    };
    let columns = columns.split(',').map(str::trim).collect::<Vec<_>>();
    let values = split_sql_values(values);
    if let Some(index) = columns.iter().position(|column| *column == "text_id")
        && let Some(value) = values.get(index).and_then(|value| sql_string_value(value))
    {
        return SqlRowKey {
            file: file.to_string(),
            table: table.trim().to_string(),
            row_id: value,
        };
    }
    fallback_sql_row_key(file, line)
}

fn fallback_sql_row_key(file: &str, line: &str) -> SqlRowKey {
    let table = line
        .strip_prefix("INSERT INTO ")
        .and_then(|rest| rest.split_once('(').map(|(table, _)| table.trim()))
        .unwrap_or("unknown");
    SqlRowKey {
        file: file.to_string(),
        table: table.to_string(),
        row_id: line.to_string(),
    }
}

fn split_sql_values(values: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = values.chars().peekable();
    let mut in_string = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\'' => {
                current.push(ch);
                if in_string && chars.peek() == Some(&'\'') {
                    current.push(chars.next().unwrap_or('\''));
                } else {
                    in_string = !in_string;
                }
            }
            ',' if !in_string => {
                out.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

fn sql_string_value(value: &str) -> Option<String> {
    let value = value.trim();
    let inner = value.strip_prefix('\'')?.strip_suffix('\'')?;
    Some(inner.replace("''", "'"))
}

fn git_show_or_empty(root: &Path, rev: &str, relative_path: &Path) -> ExoResult<String> {
    let spec = format!("{rev}:{}", git_relative_path(relative_path));
    let exists = run_git(root, &["cat-file", "-e", &spec])?;
    if !exists.status.success() {
        return Ok(String::new());
    }
    let output = run_git_checked(root, &["show", &spec], "git show")?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn fetch_remote(root: &Path, remote: &str) -> ExoResult<()> {
    run_git_checked(root, &["fetch", remote], "git fetch").map(|_| ())
}

#[derive(Debug, Clone, Copy)]
struct SidecarPushResult {
    pushed: bool,
    already_synced: bool,
}

fn push_sidecar_branch_with_recovery(
    repo: &ResolvedSidecarRepo,
    remote: &str,
    branch: &str,
) -> ExoResult<SidecarPushResult> {
    let root = &repo.sidecar_root;
    let set_upstream = !branch_has_upstream(root, branch)?;
    fetch_remote(root, remote)?;

    if remote_branch_matches_head(root, remote, branch)? {
        refresh_remote_tracking_after_push(root, remote, branch)?;
        return Ok(SidecarPushResult {
            pushed: false,
            already_synced: true,
        });
    }

    if let Some((_ahead, behind)) = ahead_behind_for_remote(root, branch, remote)?
        && behind.unwrap_or(0) > 0
    {
        return Err(sidecar_remote_updates_error(remote, branch));
    }

    let push_args = if set_upstream {
        vec!["push", "-u", remote, branch]
    } else {
        vec!["push", remote, branch]
    };
    let output = run_git(root, &push_args)?;
    if output.status.success() {
        refresh_remote_tracking_after_push(root, remote, branch)?;
        return Ok(SidecarPushResult {
            pushed: true,
            already_synced: false,
        });
    }

    fetch_remote(root, remote)?;
    if remote_branch_matches_head(root, remote, branch)? {
        refresh_remote_tracking_after_push(root, remote, branch)?;
        return Ok(SidecarPushResult {
            pushed: false,
            already_synced: true,
        });
    }
    if let Some((_ahead, behind)) = ahead_behind_for_remote(root, branch, remote)?
        && behind.unwrap_or(0) > 0
    {
        return Err(sidecar_remote_updates_error(remote, branch));
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let details = if stderr.is_empty() { stdout } else { stderr };
    anyhow::bail!("sidecar repo push failed: {details}")
}

fn remote_branch_matches_head(root: &Path, remote: &str, branch: &str) -> ExoResult<bool> {
    let remote_ref = format!("refs/remotes/{remote}/{branch}");
    let output = run_git(root, &["rev-parse", remote_ref.as_str(), "HEAD"])?;
    if !output.status.success() {
        return Ok(false);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    Ok(matches!(
        (lines.next(), lines.next()),
        (Some(remote_head), Some(local_head)) if remote_head == local_head
    ))
}

fn sidecar_remote_updates_error(remote: &str, branch: &str) -> anyhow::Error {
    anyhow::Error::new(ExoFailure::new(
        ErrorCode::PreconditionFailed,
        format!(
            "sidecar repo has updates from {remote}/{branch}; run sidecar repo sync before pushing"
        ),
        ExoFailure::orienting_steering(vec![SuggestedAction {
            label: "Sync sidecar repo".to_string(),
            command: "exo sidecar repo sync".to_string(),
            rationale: "Merge portable sidecar state before pushing new local state.".to_string(),
            intent: WorkIntent::Execute,
            confidence: Some(1.0),
        }]),
    ))
}

fn refresh_remote_tracking_after_push(root: &Path, remote: &str, branch: &str) -> ExoResult<()> {
    let remote_ref = format!("refs/remotes/{remote}/{branch}");
    run_git_checked(
        root,
        &["update-ref", &remote_ref, "HEAD"],
        "git update-ref remote tracking branch",
    )
    .map(|_| ())
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

fn ensure_sidecar_runtime_paths_untracked(repo: &ResolvedSidecarRepo) -> ExoResult<()> {
    let Some(key) = repo.project.sidecar_key.as_deref() else {
        return Ok(());
    };

    for path in [
        format!("projects/{key}/cache"),
        format!("projects/{key}/runtime"),
    ] {
        run_git_checked(
            &repo.sidecar_root,
            &["rm", "--cached", "-r", "--ignore-unmatch", &path],
            "git rm --cached",
        )?;
    }

    Ok(())
}

fn ensure_sidecar_write_ownership(
    repo: &ResolvedSidecarRepo,
) -> ExoResult<SidecarWriteOwnershipStatus> {
    let current = current_sidecar_write_owner_marker(repo)?;
    let status = read_sidecar_write_ownership_status_with_current(repo, &current)?;
    if !status.ok {
        return Err(anyhow::Error::new(ExoFailure::new(
            ErrorCode::PreconditionFailed,
            status.issue.unwrap_or_else(|| {
                "sidecar write ownership is held by another active runtime".to_string()
            }),
            ExoFailure::orienting_steering(vec![SuggestedAction {
                label: "Inspect sidecar ownership".to_string(),
                command: "exo sidecar repo status".to_string(),
                rationale: "Review which runtime owns sidecar checkpointing before retrying."
                    .to_string(),
                intent: WorkIntent::Orient,
                confidence: Some(1.0),
            }]),
        )));
    }

    write_sidecar_write_owner_marker(repo, &current)?;
    Ok(sidecar_write_ownership_status_from_marker(
        repo,
        sidecar_write_owner_marker_path(repo)?,
        Some(&current),
        "owned",
        true,
        None,
    ))
}

fn read_sidecar_write_ownership_status(
    repo: &ResolvedSidecarRepo,
) -> ExoResult<SidecarWriteOwnershipStatus> {
    let current = current_sidecar_write_owner_marker(repo)?;
    read_sidecar_write_ownership_status_with_current(repo, &current)
}

fn read_sidecar_write_ownership_status_with_current(
    repo: &ResolvedSidecarRepo,
    current: &SidecarWriteOwnerMarker,
) -> ExoResult<SidecarWriteOwnershipStatus> {
    let marker_path = sidecar_write_owner_marker_path(repo)?;
    let marker = match std::fs::read_to_string(&marker_path) {
        Ok(contents) => match serde_json::from_str::<SidecarWriteOwnerMarker>(&contents) {
            Ok(marker) => marker,
            Err(error) => {
                return Ok(sidecar_write_ownership_status_from_marker(
                    repo,
                    marker_path,
                    None,
                    "blocked",
                    false,
                    Some(format!(
                        "sidecar write ownership marker is invalid: {error}"
                    )),
                ));
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(sidecar_write_ownership_status_from_marker(
                repo,
                marker_path,
                None,
                "available",
                true,
                None,
            ));
        }
        Err(error) => {
            return Ok(sidecar_write_ownership_status_from_marker(
                repo,
                marker_path,
                None,
                "blocked",
                false,
                Some(format!(
                    "failed to read sidecar write ownership marker: {error}"
                )),
            ));
        }
    };

    let compatible = sidecar_write_owner_is_compatible(&marker, current);
    let liveness = process_liveness(marker.pid);
    let same_binary = marker.executable_blake3 == current.executable_blake3;
    let same_process = marker.pid == current.pid;
    let binary_identity_known =
        marker.executable_blake3.is_some() && current.executable_blake3.is_some();
    let process_identity_known =
        marker.process_start_id.is_some() && current.process_start_id.is_some();
    let (state, ok, issue) = if compatible && same_process && same_binary {
        ("owned", true, None)
    } else if liveness == ProcessLiveness::Dead {
        (
            "stale",
            true,
            Some(
                "sidecar write ownership belonged to a dead runtime and can be reclaimed"
                    .to_string(),
            ),
        )
    } else if compatible && !same_process && same_binary {
        (
            "blocked",
            false,
            Some("sidecar write ownership is held by another active runtime".to_string()),
        )
    } else if compatible
        && !same_process
        && !same_binary
        && binary_identity_known
        && process_identity_known
        && liveness == ProcessLiveness::Alive
        && marker.machine == current.machine
    {
        (
            "blocked",
            false,
            Some(
                "sidecar write ownership is held by a live runtime with a different executable identity for this workspace"
                    .to_string(),
            ),
        )
    } else if compatible && !same_binary {
        (
            "blocked",
            false,
            Some(
                "sidecar write ownership is held by a stale Exo runtime for this workspace"
                    .to_string(),
            ),
        )
    } else {
        (
            "blocked",
            false,
            Some("sidecar write ownership is held by another active runtime".to_string()),
        )
    };

    Ok(sidecar_write_ownership_status_from_marker(
        repo,
        marker_path,
        Some(&marker),
        state,
        ok,
        issue,
    ))
}

fn sidecar_write_ownership_status_from_marker(
    repo: &ResolvedSidecarRepo,
    marker_path: PathBuf,
    marker: Option<&SidecarWriteOwnerMarker>,
    state: &'static str,
    ok: bool,
    issue: Option<String>,
) -> SidecarWriteOwnershipStatus {
    SidecarWriteOwnershipStatus {
        kind: "sidecar.write_ownership",
        ok,
        state,
        sidecar_key: repo.project.sidecar_key.clone(),
        marker_path,
        owner_pid: marker.map(|marker| marker.pid),
        owner_workspace_root: marker.and_then(|marker| marker.workspace_root.clone()),
        owner_db_path: marker.map(|marker| marker.db_path.clone()),
        owner_binary_blake3: marker.and_then(|marker| marker.executable_blake3.clone()),
        issue,
    }
}

fn current_sidecar_write_owner_marker(
    repo: &ResolvedSidecarRepo,
) -> ExoResult<SidecarWriteOwnerMarker> {
    let sidecar_key = repo.project.sidecar_key.clone().ok_or_else(|| {
        anyhow::anyhow!("sidecar write ownership requires the current project sidecar key")
    })?;
    let now = now_ms();
    let executable_path = std::env::current_exe().ok();
    let executable_blake3 = executable_path
        .as_deref()
        .and_then(|path| file_blake3(path).ok());
    let process_start_id = process_start_identity(std::process::id()).ok();
    Ok(SidecarWriteOwnerMarker {
        version: 1,
        sidecar_key,
        sidecar_root: repo.sidecar_root.clone(),
        workspace_root: repo.project.workspace_root.clone(),
        state_root: repo.project.state_root.clone(),
        db_path: repo.project.db_path(),
        runtime_dir: repo.project.runtime_dir(),
        pid: std::process::id(),
        executable_path,
        executable_blake3,
        process_start_id,
        machine: machine_identity(),
        acquired_at_ms: now,
        refreshed_at_ms: now,
    })
}

fn write_sidecar_write_owner_marker(
    repo: &ResolvedSidecarRepo,
    marker: &SidecarWriteOwnerMarker,
) -> ExoResult<()> {
    let marker_path = sidecar_write_owner_marker_path(repo)?;
    let parent = marker_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "sidecar write ownership marker has no parent: {}",
            marker_path.display()
        )
    })?;
    std::fs::create_dir_all(parent).with_context(|| {
        format!(
            "Failed to create sidecar write ownership directory {}",
            parent.display()
        )
    })?;
    let marker_json = serde_json::to_string_pretty(marker)?;
    crate::utils::edit_file_with_permissions(&marker_path, |_| Ok(marker_json)).with_context(
        || {
            format!(
                "Failed to install sidecar write ownership marker {}",
                marker_path.display()
            )
        },
    )?;
    Ok(())
}

fn sidecar_write_owner_marker_path(repo: &ResolvedSidecarRepo) -> ExoResult<PathBuf> {
    let key = repo.project.sidecar_key.as_deref().ok_or_else(|| {
        anyhow::anyhow!("sidecar write ownership requires the current project sidecar key")
    })?;
    Ok(repo
        .sidecar_root
        .join(".git")
        .join("exo-write-owners")
        .join(format!("{}.json", sidecar_write_owner_key_fragment(key))))
}

fn sidecar_write_owner_key_fragment(key: &str) -> String {
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
    format!("{prefix}-{}", &digest[..12])
}

fn sidecar_write_owner_is_compatible(
    owner: &SidecarWriteOwnerMarker,
    current: &SidecarWriteOwnerMarker,
) -> bool {
    owner.version == current.version
        && owner.sidecar_key == current.sidecar_key
        && owner.sidecar_root == current.sidecar_root
        && owner.workspace_root == current.workspace_root
        && owner.state_root == current.state_root
        && owner.db_path == current.db_path
        && owner.runtime_dir == current.runtime_dir
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
}

fn machine_identity() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

fn file_blake3(path: &Path) -> ExoResult<String> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    reader_blake3(&mut file).with_context(|| format!("Failed to read {}", path.display()))
}

fn reader_blake3(reader: &mut impl Read) -> ExoResult<String> {
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessLiveness {
    Alive,
    Dead,
    Unknown,
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
    let Ok(output) = std::process::Command::new("ps")
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

#[cfg(target_os = "linux")]
fn process_start_identity(pid: u32) -> ExoResult<String> {
    if pid == 0 {
        return Err(anyhow::anyhow!(
            "sidecar write owner PID 0 is not a valid process identity"
        ));
    }
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).with_context(|| {
        format!("failed to read process stat for sidecar write owner process {pid}")
    })?;
    let close_paren = stat.rfind(')').ok_or_else(|| {
        anyhow::anyhow!("process stat for sidecar write owner process {pid} is malformed")
    })?;
    let start_time_ticks = stat[close_paren + 1..]
        .split_whitespace()
        .nth(19)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "process stat for sidecar write owner process {pid} is missing start time"
            )
        })?;
    Ok(format!("linux-starttime:{start_time_ticks}"))
}

#[cfg(target_os = "macos")]
fn process_start_identity(pid: u32) -> ExoResult<String> {
    if pid == 0 {
        return Err(anyhow::anyhow!(
            "sidecar write owner PID 0 is not a valid process identity"
        ));
    }
    let output = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()
        .with_context(|| {
            format!(
                "failed to resolve process start identity for sidecar write owner process {pid}"
            )
        })?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "failed to resolve process start identity for sidecar write owner process {pid}"
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| anyhow::anyhow!("process start identity is not utf-8: {error}"))?;
    let start = stdout.trim();
    if start.is_empty() {
        return Err(anyhow::anyhow!(
            "process start identity is unavailable for sidecar write owner process {pid}"
        ));
    }
    Ok(format!("macos-lstart:{start}"))
}

#[cfg(all(not(target_os = "linux"), not(target_os = "macos")))]
fn process_start_identity(pid: u32) -> ExoResult<String> {
    Err(anyhow::anyhow!(
        "process start identity is not available for sidecar write owner process {pid} on this platform"
    ))
}

#[cfg(unix)]
fn process_liveness(pid: u32) -> ProcessLiveness {
    if pid == 0 {
        return ProcessLiveness::Unknown;
    }
    if process_is_defunct(pid) {
        return ProcessLiveness::Dead;
    }
    if pid == std::process::id() {
        return ProcessLiveness::Alive;
    }
    let pid = match i32::try_from(pid) {
        Ok(pid) => pid,
        Err(_) => return ProcessLiveness::Unknown,
    };
    match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None) {
        Ok(()) => ProcessLiveness::Alive,
        Err(nix::errno::Errno::ESRCH) => ProcessLiveness::Dead,
        Err(nix::errno::Errno::EPERM) => ProcessLiveness::Alive,
        Err(_) => ProcessLiveness::Unknown,
    }
}

#[cfg(windows)]
fn process_liveness(pid: u32) -> ProcessLiveness {
    if pid == 0 {
        return ProcessLiveness::Unknown;
    }
    if pid == std::process::id() {
        return ProcessLiveness::Alive;
    }
    let output = match ProcessCommand::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
    {
        Ok(output) => output,
        Err(_) => return ProcessLiveness::Unknown,
    };
    if !output.status.success() {
        return ProcessLiveness::Unknown;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() || stdout.trim_start().starts_with("INFO:") {
        return ProcessLiveness::Dead;
    }
    let pid_text = pid.to_string();
    if stdout.lines().any(|line| {
        line.split(',')
            .nth(1)
            .map(|field| field.trim().trim_matches('"') == pid_text)
            .unwrap_or(false)
    }) {
        ProcessLiveness::Alive
    } else {
        ProcessLiveness::Dead
    }
}

#[cfg(all(not(unix), not(windows)))]
fn process_liveness(pid: u32) -> ProcessLiveness {
    if pid == std::process::id() {
        ProcessLiveness::Alive
    } else {
        ProcessLiveness::Unknown
    }
}

fn should_auto_push(policy: SidecarAutoPushPolicy, root: &Path) -> ExoResult<bool> {
    match policy {
        SidecarAutoPushPolicy::Never => Ok(false),
        SidecarAutoPushPolicy::IfRemote => Ok(first_remote(root)?.is_some()),
        SidecarAutoPushPolicy::Always => Ok(true),
    }
}

impl ResolvedSidecarRepo {
    fn project_workspace_root(&self) -> PathBuf {
        self.project
            .workspace_root
            .clone()
            .unwrap_or_else(|| self.sidecar_root.clone())
    }
}

impl ResolvedSidecarCheckpointTarget {
    fn project_workspace_root(&self) -> PathBuf {
        self.project
            .workspace_root
            .clone()
            .unwrap_or_else(|| self.sidecar_root.clone())
    }
}

fn read_status_files(root: &Path) -> ExoResult<Vec<SidecarRepoFileStatus>> {
    let output = run_git_checked(
        root,
        &["status", "--porcelain", "--untracked-files=all"],
        "git status --porcelain --untracked-files=all",
    )?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| SidecarRepoFileStatus {
            status: line.get(..2).unwrap_or(line).trim().to_string(),
            path: line.get(3..).unwrap_or_default().to_string(),
        })
        .collect())
}

fn current_branch(root: &Path) -> ExoResult<Option<String>> {
    let output = run_git(root, &["branch", "--show-current"])?;
    if !output.status.success() {
        return Ok(None);
    }
    let branch = git_stdout(&output);
    Ok((!branch.is_empty()).then_some(branch))
}

fn first_remote(root: &Path) -> ExoResult<Option<String>> {
    Ok(remote_names(root)?.into_iter().next())
}

fn first_remote_url(root: &Path) -> ExoResult<Option<String>> {
    let Some(remote) = first_remote(root)? else {
        return Ok(None);
    };
    remote_url(root, &remote)
}

fn remote_names(root: &Path) -> ExoResult<Vec<String>> {
    let output = run_git(
        root,
        &[
            "config",
            "--local",
            "--includes",
            "--get-regexp",
            r"^remote\..*\.url$",
        ],
    )?;
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let mut names = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some(key) = line.split_whitespace().next() else {
            continue;
        };
        let Some(name) = key
            .strip_prefix("remote.")
            .and_then(|value| value.strip_suffix(".url"))
        else {
            continue;
        };
        if !names.iter().any(|existing| existing == name) {
            names.push(name.to_string());
        }
    }
    names.sort();
    Ok(names)
}

fn remote_url(root: &Path, remote: &str) -> ExoResult<Option<String>> {
    let output = run_git(root, &["remote", "get-url", remote])?;
    if output.status.success() {
        return Ok(Some(git_stdout(&output)));
    }
    Ok(None)
}

fn configured_remote_url(root: &Path, remote: &str) -> ExoResult<Option<String>> {
    let key = format!("remote.{remote}.url");
    let output = run_git(root, &["config", "--local", "--includes", "--get", &key])?;
    if output.status.success() {
        return Ok(Some(git_stdout(&output)));
    }
    Ok(None)
}

fn ensure_remote_exists(root: &Path, remote: &str) -> ExoResult<()> {
    if remote_url(root, remote)?.is_some() {
        return Ok(());
    }

    Err(anyhow::Error::new(ExoFailure::new(
        ErrorCode::PreconditionFailed,
        format!("sidecar repo push requires an existing remote named '{remote}'"),
        ExoFailure::orienting_steering(vec![SuggestedAction {
            label: "Add sidecar remote".to_string(),
            command: format!("exo sidecar repo remote --remote {remote} --url <url>"),
            rationale: "Configure a git remote before pushing sidecar state.".to_string(),
            intent: WorkIntent::Execute,
            confidence: Some(1.0),
        }]),
    )))
}

#[derive(Debug, Clone, Copy)]
struct SidecarUpstreamRelation {
    ahead: Option<u32>,
    behind: Option<u32>,
    has_merge_base: bool,
}

fn read_upstream_relation(root: &Path, branch: &str) -> ExoResult<Option<SidecarUpstreamRelation>> {
    read_upstream_relation_with_remote(root, branch, None)
}

fn read_upstream_relation_with_remote(
    root: &Path,
    branch: &str,
    fallback_remote: Option<&str>,
) -> ExoResult<Option<SidecarUpstreamRelation>> {
    let Some(upstream) = upstream_ref_for_branch(root, branch, fallback_remote)? else {
        return Ok(None);
    };
    let range = format!("{upstream}...{branch}");
    let output = run_git_checked(
        root,
        &["rev-list", "--left-right", "--count", &range],
        "git rev-list --left-right --count",
    )?;
    let stdout = git_stdout(&output);
    let mut parts = stdout.split_whitespace();
    let behind = parts.next().and_then(|part| part.parse::<u32>().ok());
    let ahead = parts.next().and_then(|part| part.parse::<u32>().ok());
    let merge_base = run_git(root, &["merge-base", "HEAD", &upstream])?;
    let has_merge_base = if merge_base.status.success() {
        true
    } else {
        let stderr = String::from_utf8_lossy(&merge_base.stderr)
            .trim()
            .to_string();
        if stderr.is_empty() {
            false
        } else {
            anyhow::bail!("git merge-base failed in {}: {stderr}", root.display());
        }
    };
    Ok(Some(SidecarUpstreamRelation {
        ahead,
        behind,
        has_merge_base,
    }))
}

fn ahead_behind_for_remote(
    root: &Path,
    branch: &str,
    fallback_remote: &str,
) -> ExoResult<Option<(Option<u32>, Option<u32>)>> {
    Ok(
        read_upstream_relation_with_remote(root, branch, Some(fallback_remote))?
            .map(|relation| (relation.ahead, relation.behind)),
    )
}

fn branch_has_upstream(root: &Path, branch: &str) -> ExoResult<bool> {
    let upstream = format!("{branch}@{{upstream}}");
    let output = run_git(root, &["rev-parse", "--abbrev-ref", &upstream])?;
    Ok(output.status.success())
}

fn upstream_ref_for_branch(
    root: &Path,
    branch: &str,
    fallback_remote: Option<&str>,
) -> ExoResult<Option<String>> {
    if branch_has_upstream(root, branch)? {
        return Ok(Some(format!("{branch}@{{upstream}}")));
    }
    match fallback_remote {
        Some(remote) => same_named_remote_branch(root, remote, branch),
        None => {
            let Some(remote) = first_remote(root)? else {
                return Ok(None);
            };
            same_named_remote_branch(root, &remote, branch)
        }
    }
}

fn same_named_remote_branch(root: &Path, remote: &str, branch: &str) -> ExoResult<Option<String>> {
    let remote_branch = format!("refs/remotes/{remote}/{branch}");
    let output = run_git(root, &["rev-parse", "--verify", "--quiet", &remote_branch])?;
    Ok(output.status.success().then_some(remote_branch))
}

fn run_git(root: &Path, args: &[&str]) -> ExoResult<Output> {
    ProcessCommand::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(Into::into)
}

fn shell_quote_arg(raw: &str) -> String {
    format!("'{}'", raw.replace('\'', "'\\''"))
}

fn run_git_checked(root: &Path, args: &[&str], label: &str) -> ExoResult<Output> {
    let output = run_git(root, args)?;
    if output.status.success() {
        return Ok(output);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let details = if stderr.is_empty() { stdout } else { stderr };
    anyhow::bail!("{label} failed in {}: {details}", root.display())
}

fn git_stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SidecarUnlink;

impl SidecarUnlink {
    pub const fn new() -> Self {
        Self
    }
}

#[derive(Debug, Serialize)]
struct SidecarUnlinkOutput {
    kind: &'static str,
    ok: bool,
    removed: bool,
    project_id: Option<String>,
    config_path: Option<PathBuf>,
}

impl Command for SidecarUnlink {
    fn namespace(&self) -> &'static str {
        "sidecar"
    }

    fn operation(&self) -> &'static str {
        "unlink"
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let resolver = project_resolver_for_context(ctx.project);
        let removed = unlink_sidecar_with_resolver(ctx.root, &resolver)?;
        let output = SidecarUnlinkOutput {
            kind: "sidecar.unlink",
            ok: true,
            removed: removed.is_some(),
            project_id: removed.as_ref().map(|(id, _)| id.as_str().to_string()),
            config_path: removed.map(|(_, path)| path),
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let message = if output.removed {
                    "Sidecar unlinked"
                } else {
                    "Sidecar was not linked"
                };
                Ok(CommandOutput::new(output, message))
            }
        }
    }

    fn effect(&self) -> Effect {
        Effect::Write
    }

    fn description(&self) -> &'static str {
        "Remove this repo's local sidecar binding"
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn process_liveness_treats_unreaped_child_as_dead() {
        let mut child = ProcessCommand::new("sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("spawn exiting child");
        let pid = child.id();

        for _ in 0..20 {
            if process_is_defunct(pid) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert_eq!(process_liveness(pid), ProcessLiveness::Dead);
        child.wait().expect("reap child");
    }
}

#[cfg(test)]
mod remote_order_tests {
    use super::*;

    #[test]
    fn remote_names_match_git_remote_order_when_config_insertion_differs() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let repo = temp.path();
        git_ok(repo, &["init"]);
        git_ok(
            repo,
            &[
                "remote",
                "add",
                "upstream",
                "https://github.com/upstream/repo.git",
            ],
        );
        git_ok(
            repo,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/origin/repo.git",
            ],
        );

        assert_eq!(
            remote_names(repo).expect("read remote names"),
            vec!["origin".to_string(), "upstream".to_string()]
        );
    }

    #[test]
    fn remote_names_include_remotes_from_included_local_config() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let repo = temp.path();
        git_ok(repo, &["init"]);
        let include_path = repo.join("remotes.inc");
        std::fs::write(
            &include_path,
            "[remote \"origin\"]\n\turl = https://github.com/origin/repo.git\n",
        )
        .expect("write include config");
        git_ok(
            repo,
            &[
                "config",
                "--local",
                "include.path",
                include_path.to_str().expect("utf-8 include path"),
            ],
        );

        assert_eq!(
            remote_names(repo).expect("read remote names"),
            vec!["origin".to_string()]
        );
    }

    #[test]
    fn configured_remote_url_reads_included_local_config() {
        let temp = tempfile::tempdir().expect("create tempdir");
        let repo = temp.path();
        git_ok(repo, &["init"]);
        let include_path = repo.join("remotes.inc");
        std::fs::write(
            &include_path,
            "[remote \"origin\"]\n\turl = https://github.com/origin/repo.git\n",
        )
        .expect("write include config");
        git_ok(
            repo,
            &[
                "config",
                "--local",
                "include.path",
                include_path.to_str().expect("utf-8 include path"),
            ],
        );

        assert_eq!(
            configured_remote_url(repo, "origin").expect("read configured remote url"),
            Some("https://github.com/origin/repo.git".to_string())
        );
    }

    fn git_ok(root: &Path, args: &[&str]) {
        let output = ProcessCommand::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
