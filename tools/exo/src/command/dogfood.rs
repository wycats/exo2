//! Dogfood lifecycle probes.
//!
//! `dogfood verify` is intentionally pure: every activation surface can call it
//! and compare the same binary, project, runtime, and sidecar identity.

use super::traits::{
    Command, CommandBox, CommandContext, CommandOutput, MutableCommand, MutableCommandContext,
    OutputFormat,
};
use crate::api::protocol::Effect;
use crate::command::sidecar::{SidecarAutoPersistReport, SidecarRepoSyncStatus};
use crate::daemon::{
    DaemonEnsureOutcome, DaemonEnsureState, DaemonStatusState, daemon_status_for_project,
    ensure_daemon_with_report,
};
use crate::dogfood_activation::{DOGFOOD_ACTIVATION_ENV, DogfoodActivation};
use crate::mcp::MCP_WORKER_PROTOCOL_VERSION;
use crate::project::{Project, StatePolicy};
use anyhow::{Context, Result as ExoResult, anyhow, bail};
use exosuit_storage::rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PROXY_HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, exospec::ExoSpec)]
#[exo(namespace = "dogfood", description = "Dogfood activation probes")]
pub enum DogfoodCommands {
    #[exo(
        effect = "pure",
        description = "Verify Exosuit dogfood activation identity and state paths"
    )]
    Verify {
        #[exo(
            flag,
            description = "Skip saved activation baseline comparison for pre-baseline checks"
        )]
        skip_receipt: bool,
        #[exo(
            flag,
            description = "Require the workspace daemon socket to be reachable"
        )]
        require_daemon: bool,
        #[exo(
            long,
            optional,
            description = "VS Code extension build stamp, when invoked by the extension"
        )]
        extension_build_stamp: Option<String>,
        #[exo(
            long,
            optional,
            description = "VS Code extension installation path, when invoked by the extension"
        )]
        extension_path: Option<String>,
    },

    #[exo(
        effect = "pure",
        description = "Preview guided repair for divergent dogfood sidecar state"
    )]
    Repair {
        #[exo(flag, description = "Apply the guided repair after confirmation")]
        apply: bool,
    },

    #[exo(
        effect = "exec",
        description = "Refresh Exo runtime health without terminating durable transports"
    )]
    Restart,

    #[exo(
        effect = "exec",
        description = "Save the dogfood activation baseline for this workspace"
    )]
    Receipt,
}

impl DogfoodCommands {
    #[allow(unused_variables)]
    pub fn to_command_box(self, root: &std::path::Path) -> anyhow::Result<CommandBox> {
        Ok(match self {
            Self::Verify {
                skip_receipt,
                require_daemon,
                extension_build_stamp,
                extension_path,
            } => CommandBox::pure(DogfoodVerify::new(
                skip_receipt,
                require_daemon,
                extension_build_stamp,
                extension_path.map(PathBuf::from),
            )),
            Self::Repair { apply } if apply => CommandBox::mutable(DogfoodRepair::new(true)),
            Self::Repair { apply } => CommandBox::pure(DogfoodRepair::new(apply)),
            Self::Restart => CommandBox::mutable(DogfoodRestartRuntimes),
            Self::Receipt => CommandBox::mutable(DogfoodReceipt::default()),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct DogfoodVerify {
    skip_receipt: bool,
    require_daemon: bool,
    extension_build_stamp: Option<String>,
    extension_path: Option<PathBuf>,
}

impl DogfoodVerify {
    pub const fn new(
        skip_receipt: bool,
        require_daemon: bool,
        extension_build_stamp: Option<String>,
        extension_path: Option<PathBuf>,
    ) -> Self {
        Self {
            skip_receipt,
            require_daemon,
            extension_build_stamp,
            extension_path,
        }
    }
}

impl Command for DogfoodVerify {
    fn namespace(&self) -> &'static str {
        "dogfood"
    }

    fn operation(&self) -> &'static str {
        "verify"
    }

    fn description(&self) -> &'static str {
        "Verify Exosuit dogfood activation identity and state paths"
    }

    fn effect(&self) -> Effect {
        Effect::Pure
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let project = ctx
            .project
            .cloned()
            .map_or_else(|| Project::resolve(ctx.root), Ok)?;
        let mut output = DogfoodVerifyOutput::from_project(
            ctx.root,
            &project,
            self.require_daemon,
            self.extension_build_stamp
                .clone()
                .map(|build_stamp| ExtensionIdentity {
                    build_stamp,
                    path: self.extension_path.clone(),
                    bundle_sha256: None,
                    bundle_size_bytes: None,
                    manifest_path: None,
                }),
            None,
        )?;
        output.receipt_skipped = self.skip_receipt;
        if !self.skip_receipt {
            output.compare_receipt()?;
        }
        output.ok = output.split_brain.errors == 0
            && output.daemon.ok
            && output.portability.ok
            && output.plugin.as_ref().is_none_or(|plugin| plugin.ok)
            && output
                .receipt
                .as_ref()
                .is_none_or(|receipt| receipt.matches);

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let status = if output.ok { "passed" } else { "failed" };
                let mut message = format!(
                    "Dogfood activation {status}\n  binary: {}\n  db: {}\n",
                    output.binary.path.display(),
                    output.paths.db_path.display()
                );
                if let Some(receipt) = &output.receipt {
                    message.push_str(&format!(
                        "  saved activation baseline: {} ({})\n",
                        receipt.path.display(),
                        if receipt.matches {
                            "matches"
                        } else {
                            "mismatch"
                        }
                    ));
                }
                if output.split_brain.errors > 0 {
                    message.push_str("  split-brain: repair required\n");
                }
                if !output.daemon.ok
                    && let Some(issue) = &output.daemon.issue
                {
                    message.push_str(&format!("  daemon: {issue}\n"));
                }
                if let Some(plugin) = &output.plugin
                    && !plugin.ok
                    && let Some(issue) = &plugin.issue
                {
                    message.push_str(&format!("  plugin: {issue}\n"));
                }
                Ok(CommandOutput::new(output, message))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DogfoodRepair {
    apply: bool,
}

impl DogfoodRepair {
    pub const fn new(apply: bool) -> Self {
        Self { apply }
    }
}

impl Command for DogfoodRepair {
    fn namespace(&self) -> &'static str {
        "dogfood"
    }

    fn operation(&self) -> &'static str {
        "repair"
    }

    fn description(&self) -> &'static str {
        "Preview guided repair for divergent dogfood sidecar state"
    }

    fn effect(&self) -> Effect {
        if self.apply {
            Effect::Exec
        } else {
            Effect::Pure
        }
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        if self.apply {
            unreachable!("DogfoodRepair --apply should be dispatched via execute_mut");
        }
        let project = ctx
            .project
            .cloned()
            .map_or_else(|| Project::resolve(ctx.root), Ok)?;
        let paths = ProjectPaths::from_project(&project);
        let split_brain = SplitBrainReport::scan(&project, &paths.db_path)?;
        let repair_plan = RepairPlan::build(&paths, &split_brain)?;
        let output = DogfoodRepairPreviewOutput {
            kind: "dogfood.repair.preview",
            ok: split_brain.errors == 0,
            preview: true,
            canonical_db: split_brain.canonical_db.clone(),
            candidates: split_brain.candidates.clone(),
            plan: repair_plan,
            requires_confirmation_before_migration: split_brain.errors > 0,
            migration_implemented: true,
            note: if split_brain.errors > 0 {
                "Divergent sidecar DBs were detected. Run `exo dogfood repair --apply` after reviewing this preview."
            } else {
                "No repair-required sidecar split-brain was detected."
            },
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let mut message = if output.ok {
                    "No repair-required dogfood split-brain was detected.\n".to_string()
                } else {
                    "Dogfood split-brain repair preview:\n".to_string()
                };
                for candidate in &output.candidates {
                    if matches!(candidate.severity, SplitBrainSeverity::Error) {
                        message.push_str(&format!(
                            "  {}: {} ({} rows)\n",
                            candidate.kind,
                            candidate.db_path.display(),
                            candidate.db.exo_rows
                        ));
                    }
                }
                Ok(CommandOutput::new(output, message))
            }
        }
    }
}

impl MutableCommand for DogfoodRepair {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        if !self.apply {
            unreachable!("DogfoodRepair preview should be dispatched via execute");
        }

        let project = ctx
            .project
            .cloned()
            .map_or_else(|| Project::resolve(ctx.root), Ok)?;
        let paths = ProjectPaths::from_project(&project);
        let split_brain = SplitBrainReport::scan(&project, &paths.db_path)?;
        let plan = RepairPlan::build(&paths, &split_brain)?;
        let applied = apply_repair_plan(ctx.root, &project, &paths, &plan)?;
        let output = DogfoodRepairApplyOutput {
            kind: "dogfood.repair.apply",
            ok: applied.ok,
            preview: false,
            canonical_db: DbSummary::inspect(&paths.db_path)?,
            plan,
            applied,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                "Applied dogfood split-brain repair.",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DogfoodRestartRuntimes;

impl Command for DogfoodRestartRuntimes {
    fn namespace(&self) -> &'static str {
        "dogfood"
    }

    fn operation(&self) -> &'static str {
        "restart"
    }

    fn description(&self) -> &'static str {
        "Refresh Exo runtime health without terminating durable transports"
    }

    fn effect(&self) -> Effect {
        Effect::Exec
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let mut mutable_ctx = MutableCommandContext {
            root: ctx.root,
            project: ctx.project,
            format: ctx.format,
            agent_id: ctx.agent_id.clone(),
            workflow_confirmation: ctx.workflow_confirmation.clone(),
        };
        self.execute_mut(&mut mutable_ctx)
    }
}

impl MutableCommand for DogfoodRestartRuntimes {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let project = ctx
            .project
            .cloned()
            .map_or_else(|| Project::resolve(ctx.root), Ok)?;
        let daemon = ensure_daemon_runtime(ctx.root, &project)?;
        let mcp = inspect_workspace_mcp_servers(ctx.root)?;
        let output = DogfoodRestartRuntimesOutput {
            kind: "dogfood.restart-runtimes",
            ok: true,
            daemon,
            mcp,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => Ok(CommandOutput::new(
                output,
                "Refreshed Exo runtime health without terminating durable transports.",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DogfoodReceipt;

impl Command for DogfoodReceipt {
    fn namespace(&self) -> &'static str {
        "dogfood"
    }

    fn operation(&self) -> &'static str {
        "receipt"
    }

    fn description(&self) -> &'static str {
        "Save the dogfood activation baseline for this workspace"
    }

    fn effect(&self) -> Effect {
        Effect::Exec
    }

    fn execute(&self, ctx: &CommandContext) -> ExoResult<CommandOutput> {
        let mut mutable_ctx = MutableCommandContext {
            root: ctx.root,
            project: ctx.project,
            format: ctx.format,
            agent_id: ctx.agent_id.clone(),
            workflow_confirmation: ctx.workflow_confirmation.clone(),
        };
        self.execute_mut(&mut mutable_ctx)
    }
}

impl MutableCommand for DogfoodReceipt {
    fn execute_mut(&self, ctx: &mut MutableCommandContext) -> ExoResult<CommandOutput> {
        let project = ctx
            .project
            .cloned()
            .map_or_else(|| Project::resolve(ctx.root), Ok)?;
        let extension_manifest = ctx
            .root
            .join("packages/exosuit-vscode/out/dev-host-bundle.json");
        let extension = if extension_manifest.exists() {
            Some(ExtensionIdentity::from_manifest(&extension_manifest)?)
        } else {
            None
        };
        let mut receipt =
            DogfoodVerifyOutput::from_project(ctx.root, &project, false, extension, None)?;
        if receipt.split_brain.errors > 0 {
            bail!(
                "Refusing to save dogfood activation baseline while split-brain repair is required. Run `exo dogfood repair` first."
            );
        }
        if let Some(plugin) = &receipt.plugin
            && !plugin.ok
        {
            let issue = plugin
                .issue
                .as_deref()
                .unwrap_or("plugin health is failing");
            bail!(
                "Refusing to save dogfood activation baseline while plugin health is failing: {issue}"
            );
        }
        receipt.ok =
            receipt.portability.ok && receipt.plugin.as_ref().is_none_or(|plugin| plugin.ok);

        fs::create_dir_all(&receipt.paths.runtime_dir).with_context(|| {
            format!(
                "Failed to create dogfood runtime dir {}",
                receipt.paths.runtime_dir.display()
            )
        })?;
        let path = receipt.receipt_path.clone();
        fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&receipt)?),
        )
        .with_context(|| {
            format!(
                "Failed to write dogfood activation baseline {}",
                path.display()
            )
        })?;

        let output = DogfoodReceiptOutput {
            kind: "dogfood.receipt",
            ok: receipt.ok,
            receipt_path: path,
            split_brain_errors: receipt.split_brain.errors,
            split_brain_warnings: receipt.split_brain.warnings,
        };

        match ctx.format {
            OutputFormat::Json => Ok(CommandOutput::data(output)),
            OutputFormat::Human => {
                let message = format!(
                    "Saved dogfood activation baseline: {}",
                    output.receipt_path.display()
                );
                Ok(CommandOutput::new(output, message))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DogfoodVerifyOutput {
    kind: &'static str,
    ok: bool,
    binary: BinaryIdentity,
    extension: Option<ExtensionIdentity>,
    plugin: Option<PluginIdentity>,
    project: ProjectIdentity,
    paths: ProjectPaths,
    daemon: DaemonIdentity,
    sidecar: SidecarIdentity,
    portability: DogfoodPortability,
    split_brain: SplitBrainReport,
    receipt_path: PathBuf,
    receipt_skipped: bool,
    receipt: Option<ReceiptComparison>,
    repair: RepairGuidance,
}

#[derive(Debug, Clone, Serialize)]
struct DogfoodRepairPreviewOutput {
    kind: &'static str,
    ok: bool,
    preview: bool,
    canonical_db: DbSummary,
    candidates: Vec<SplitBrainCandidate>,
    plan: RepairPlan,
    requires_confirmation_before_migration: bool,
    migration_implemented: bool,
    note: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct DogfoodRepairApplyOutput {
    kind: &'static str,
    ok: bool,
    preview: bool,
    canonical_db: DbSummary,
    plan: RepairPlan,
    applied: RepairApplyReport,
}

#[derive(Debug, Clone, Serialize)]
struct DogfoodRestartRuntimesOutput {
    kind: &'static str,
    ok: bool,
    daemon: RuntimeRestartReport,
    mcp: McpRestartReport,
}

#[derive(Debug, Clone, Serialize)]
struct DogfoodReceiptOutput {
    kind: &'static str,
    ok: bool,
    receipt_path: PathBuf,
    split_brain_errors: usize,
    split_brain_warnings: usize,
}

impl DogfoodVerifyOutput {
    fn from_project(
        root: &Path,
        project: &Project,
        require_daemon: bool,
        extension: Option<ExtensionIdentity>,
        plugin: Option<PluginIdentity>,
    ) -> ExoResult<Self> {
        let binary = BinaryIdentity::current()?;
        let plugin = match plugin {
            Some(plugin) => Some(plugin),
            None => default_plugin_identity(root, &binary)?,
        };
        let paths = ProjectPaths::from_project(project);
        let receipt_path = paths.runtime_dir.join("dogfood-receipt.json");
        let split_brain = SplitBrainReport::scan(project, &paths.db_path)?;
        let sidecar = SidecarIdentity::from_project(project, &split_brain);
        let portability = DogfoodPortability::from_project(root, project);
        let repair = RepairGuidance::from_split_brain(&split_brain);

        Ok(Self {
            kind: "dogfood.verify",
            ok: false,
            binary,
            extension,
            plugin,
            project: ProjectIdentity::from_project(project),
            daemon: DaemonIdentity::from_project(root, project, require_daemon),
            sidecar,
            portability,
            paths,
            split_brain,
            receipt_path,
            receipt_skipped: false,
            receipt: None,
            repair,
        })
    }

    fn compare_receipt(&mut self) -> ExoResult<()> {
        if !self.receipt_path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&self.receipt_path)
            .with_context(|| format!("Failed to read {}", self.receipt_path.display()))?;
        let expected: ReceiptExpected = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", self.receipt_path.display()))?;
        let mut mismatches = Vec::new();

        compare_path(
            "binary.path",
            Some(&expected.binary.path),
            &self.binary.path,
            &mut mismatches,
        );
        compare_string(
            "binary.blake3",
            Some(expected.binary.blake3.as_str()),
            &self.binary.blake3,
            &mut mismatches,
        );
        compare_string(
            "project.id",
            Some(expected.project.id.as_str()),
            &self.project.id,
            &mut mismatches,
        );
        compare_path(
            "paths.state_root",
            Some(&expected.paths.state_root),
            &self.paths.state_root,
            &mut mismatches,
        );
        compare_path(
            "paths.db_path",
            Some(&expected.paths.db_path),
            &self.paths.db_path,
            &mut mismatches,
        );
        compare_path(
            "paths.runtime_dir",
            Some(&expected.paths.runtime_dir),
            &self.paths.runtime_dir,
            &mut mismatches,
        );
        compare_path(
            "paths.socket_path",
            Some(&expected.paths.socket_path),
            &self.paths.socket_path,
            &mut mismatches,
        );

        if let (Some(expected_extension), Some(current_extension)) =
            (expected.extension.as_ref(), self.extension.as_ref())
        {
            compare_string(
                "extension.build_stamp",
                Some(expected_extension.build_stamp.as_str()),
                &current_extension.build_stamp,
                &mut mismatches,
            );
            compare_optional_string(
                "extension.bundle_sha256",
                expected_extension.bundle_sha256.as_deref(),
                current_extension.bundle_sha256.as_deref(),
                &mut mismatches,
            );
        }

        if let (Some(expected_plugin), Some(current_plugin)) =
            (expected.plugin.as_ref(), self.plugin.as_ref())
        {
            compare_string(
                "plugin.blake3",
                Some(expected_plugin.blake3.as_str()),
                &current_plugin.blake3,
                &mut mismatches,
            );
            if let (Some(expected_proxy), Some(current_proxy)) = (
                expected_plugin.proxy_binary.as_ref(),
                current_plugin.proxy_binary.as_ref(),
            ) {
                compare_string(
                    "plugin.proxy_binary.command",
                    Some(expected_proxy.command.as_str()),
                    &current_proxy.command,
                    &mut mismatches,
                );
                compare_optional_path(
                    "plugin.proxy_binary.path",
                    expected_proxy.path.as_ref(),
                    current_proxy.path.as_ref(),
                    &mut mismatches,
                );
                compare_optional_string(
                    "plugin.proxy_binary.source",
                    expected_proxy.source.as_deref(),
                    current_proxy.source,
                    &mut mismatches,
                );
                compare_bool(
                    "plugin.proxy_binary.executable",
                    Some(expected_proxy.executable),
                    current_proxy.executable,
                    &mut mismatches,
                );
                compare_optional_string(
                    "plugin.proxy_binary.blake3",
                    expected_proxy.blake3.as_deref(),
                    current_proxy.blake3.as_deref(),
                    &mut mismatches,
                );
                compare_optional_u64(
                    "plugin.proxy_binary.size_bytes",
                    expected_proxy.size_bytes,
                    current_proxy.size_bytes,
                    &mut mismatches,
                );
            }
        }

        self.receipt = Some(ReceiptComparison {
            present: true,
            path: self.receipt_path.clone(),
            matches: mismatches.is_empty(),
            mismatches,
        });

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
struct BinaryIdentity {
    path: PathBuf,
    blake3: String,
    size_bytes: u64,
    modified_unix_ms: Option<u128>,
}

impl BinaryIdentity {
    fn current() -> ExoResult<Self> {
        let path = std::env::current_exe()
            .context("Failed to resolve current executable")?
            .canonicalize()
            .context("Failed to canonicalize current executable")?;
        let metadata = fs::metadata(&path)
            .with_context(|| format!("Failed to stat executable {}", path.display()))?;
        let blake3 = file_blake3(&path)?;

        Ok(Self {
            path,
            blake3,
            size_bytes: metadata.len(),
            modified_unix_ms: metadata.modified().ok().and_then(system_time_ms),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
struct ExtensionIdentity {
    build_stamp: String,
    path: Option<PathBuf>,
    bundle_sha256: Option<String>,
    bundle_size_bytes: Option<u64>,
    manifest_path: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtensionBundleManifest {
    build_stamp: String,
    bundle_sha256: String,
    bundle_size_bytes: u64,
}

impl ExtensionIdentity {
    fn from_manifest(path: &Path) -> ExoResult<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read extension manifest {}", path.display()))?;
        let manifest: ExtensionBundleManifest = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse extension manifest {}", path.display()))?;
        Ok(Self {
            build_stamp: manifest.build_stamp,
            path: None,
            bundle_sha256: Some(manifest.bundle_sha256),
            bundle_size_bytes: Some(manifest.bundle_size_bytes),
            manifest_path: Some(path.to_path_buf()),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
struct PluginIdentity {
    path: PathBuf,
    blake3: String,
    files: Vec<PathBuf>,
    mcp_server: Option<PluginMcpServerIdentity>,
    proxy_binary: Option<PluginProxyBinaryIdentity>,
    ok: bool,
    issue: Option<String>,
}

impl PluginIdentity {
    fn from_dir(root: &Path, path: &Path, current_binary: &BinaryIdentity) -> ExoResult<Self> {
        let canonical = path
            .canonicalize()
            .with_context(|| format!("Failed to canonicalize plugin dir {}", path.display()))?;
        let mut files = Vec::new();
        collect_files(&canonical, &mut files)?;
        files.sort();

        let mut hasher = blake3::Hasher::new();
        let mut relative_files = Vec::new();
        for file in &files {
            let relative = file.strip_prefix(&canonical).unwrap_or(file.as_path());
            hasher.update(relative.as_os_str().as_encoded_bytes());
            hasher.update(&[0]);
            hasher.update(file_blake3(file)?.as_bytes());
            hasher.update(&[0]);
            relative_files.push(relative.to_path_buf());
        }

        let (mcp_server, proxy_binary, issue) =
            plugin_mcp_server_identity(&canonical, root, current_binary);

        Ok(Self {
            path: canonical,
            blake3: hasher.finalize().to_hex().to_string(),
            files: relative_files,
            ok: issue.is_none(),
            issue,
            mcp_server,
            proxy_binary,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
struct PluginMcpServerIdentity {
    name: String,
    command: String,
    args: Vec<String>,
    proxy_backed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct PluginProxyBinaryIdentity {
    command: String,
    path: Option<PathBuf>,
    source: Option<&'static str>,
    executable: bool,
    blake3: Option<String>,
    size_bytes: Option<u64>,
    modified_unix_ms: Option<u128>,
    activation: Option<DogfoodActivationProbe>,
    issue: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct DogfoodActivationProbe {
    configured: bool,
    ok: bool,
    state: String,
    issue: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PluginMcpConfig {
    #[serde(rename = "mcpServers")]
    mcp_servers: BTreeMap<String, PluginMcpServerConfig>,
}

#[derive(Debug, Deserialize)]
struct PluginMcpServerConfig {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

fn plugin_mcp_server_identity(
    plugin_dir: &Path,
    root: &Path,
    current_binary: &BinaryIdentity,
) -> (
    Option<PluginMcpServerIdentity>,
    Option<PluginProxyBinaryIdentity>,
    Option<String>,
) {
    let path = plugin_dir.join(".mcp.json");
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (None, None, Some("plugin .mcp.json is missing".to_string()));
        }
        Err(error) => {
            return (
                None,
                None,
                Some(format!(
                    "failed to read plugin MCP config {}: {error}",
                    path.display()
                )),
            );
        }
    };
    let config = match serde_json::from_str::<PluginMcpConfig>(&content) {
        Ok(config) => config,
        Err(error) => {
            return (
                None,
                None,
                Some(format!(
                    "failed to parse plugin MCP config {}: {error}",
                    path.display()
                )),
            );
        }
    };
    let Some(server) = config.mcp_servers.get("exo") else {
        return (
            None,
            None,
            Some("plugin MCP config is missing the exo server".to_string()),
        );
    };
    let proxy_backed = is_exo_mcp_command(&server.command) && server.args.is_empty();
    let identity = PluginMcpServerIdentity {
        name: "exo".to_string(),
        command: server.command.clone(),
        args: server.args.clone(),
        proxy_backed,
    };
    if !proxy_backed {
        return (
            Some(identity),
            None,
            Some("plugin MCP server must launch exo-mcp with no args".to_string()),
        );
    }

    let proxy_binary =
        plugin_proxy_binary_identity(root, &server.command, current_binary, &server.env);
    let issue = proxy_binary.issue.clone();
    (Some(identity), Some(proxy_binary), issue)
}

fn plugin_proxy_binary_identity(
    root: &Path,
    command: &str,
    current_binary: &BinaryIdentity,
    env: &BTreeMap<String, String>,
) -> PluginProxyBinaryIdentity {
    let command_path = Path::new(command);
    if command_path.is_absolute() {
        return plugin_proxy_binary_for_path(
            command,
            command_path.to_path_buf(),
            "plugin-command",
            current_binary,
            env,
        );
    }

    let target = root.join("target").join("debug").join(binary_name(command));
    let workspace_target = target
        .exists()
        .then(|| target.canonicalize().unwrap_or(target));

    if let Some(path) = find_command_in_path(command) {
        return plugin_proxy_binary_for_path(command, path, "path", current_binary, env);
    }

    let workspace_note = if let Some(workspace_target) = workspace_target {
        format!(
            "; a workspace proxy exists at {}, but the plugin host resolves `{command}` through PATH",
            workspace_target.display()
        )
    } else {
        String::new()
    };

    PluginProxyBinaryIdentity {
        command: command.to_string(),
        path: None,
        source: None,
        executable: false,
        blake3: None,
        size_bytes: None,
        modified_unix_ms: None,
        activation: None,
        issue: Some(format!(
            "plugin MCP server command `{command}` was not found on PATH{workspace_note}; run `cargo install --path tools/exo --locked`"
        )),
    }
}

fn plugin_proxy_binary_for_path(
    command: &str,
    path: PathBuf,
    source: &'static str,
    current_binary: &BinaryIdentity,
    env: &BTreeMap<String, String>,
) -> PluginProxyBinaryIdentity {
    let mut path = path.canonicalize().unwrap_or(path);
    let mut source = source;
    let executable = is_executable(&path);
    let metadata;
    let mut blake3 = file_blake3(&path).ok();
    let mut health = None;
    let issue = if !executable {
        Some(format!(
            "plugin MCP server command `{command}` resolved to {} but it is not executable; run `cargo install --path tools/exo --locked`",
            path.display()
        ))
    } else if blake3.is_none() {
        Some(format!(
            "plugin MCP server command `{command}` resolved to {} but dogfood verify could not hash it",
            path.display()
        ))
    } else {
        health = Some(proxy_health_probe(command, &path, env));
        if let Some(issue) = health.as_ref().and_then(|health| health.issue.clone()) {
            Some(issue)
        } else if let Some(activation) = health
            .as_ref()
            .and_then(|health| health.activation.as_ref())
            && activation.configured
            && !activation.ok
        {
            Some(activation.issue.clone().unwrap_or_else(|| {
                "the Exo MCP proxy dogfood activation is not current; run `cargo dogfood-exo` from the source checkout".to_string()
            }))
        } else if health
            .as_ref()
            .and_then(|health| health.worker.as_ref())
            .is_none()
        {
            Some(format!(
                "plugin MCP server command `{command}` resolved to {} but proxy health did not report worker identity",
                path.display()
            ))
        } else if let Some(worker) = health.as_ref().and_then(|health| health.worker.as_ref())
            && !health
                .as_ref()
                .and_then(|health| health.activation.as_ref())
                .is_some_and(|activation| activation.configured && activation.ok)
            && worker.blake3 != current_binary.blake3
        {
            Some(format!(
                "plugin MCP server command `{command}` resolved to {} but proxy health routes through worker {} with a different executable hash than the current exo binary; run `cargo install --path tools/exo --locked`",
                path.display(),
                worker.path.display()
            ))
        } else {
            None
        }
    };

    if issue.is_none()
        && let Some(effective_proxy) = health.as_ref().and_then(|health| health.proxy.as_ref())
    {
        path = effective_proxy.path.clone();
        source = "proxy-health";
        blake3 = Some(effective_proxy.blake3.clone());
        metadata = None;
    } else {
        metadata = fs::metadata(&path).ok();
    }

    PluginProxyBinaryIdentity {
        command: command.to_string(),
        path: Some(path),
        source: Some(source),
        executable,
        blake3,
        size_bytes: health
            .as_ref()
            .and_then(|health| health.proxy.as_ref())
            .and_then(|proxy| proxy.size_bytes)
            .or_else(|| metadata.as_ref().map(fs::Metadata::len)),
        modified_unix_ms: health
            .as_ref()
            .and_then(|health| health.proxy.as_ref())
            .and_then(|proxy| proxy.modified_unix_ms)
            .or_else(|| {
                metadata
                    .as_ref()
                    .and_then(|metadata| metadata.modified().ok())
                    .and_then(system_time_ms)
            }),
        activation: health.and_then(|health| health.activation),
        issue,
    }
}

#[derive(Debug, Default)]
struct ProxyHealthProbe {
    issue: Option<String>,
    proxy: Option<ProbeExecutableIdentity>,
    worker: Option<ProbeExecutableIdentity>,
    activation: Option<DogfoodActivationProbe>,
}

#[derive(Debug)]
struct ProbeExecutableIdentity {
    path: PathBuf,
    blake3: String,
    size_bytes: Option<u64>,
    modified_unix_ms: Option<u128>,
}

fn proxy_health_probe(
    command: &str,
    path: &Path,
    env: &BTreeMap<String, String>,
) -> ProxyHealthProbe {
    let output_path = std::env::temp_dir().join(format!(
        "exo-proxy-health-{}-{}-{}.json",
        std::process::id(),
        uuid::Uuid::new_v4(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let output_file = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_path)
    {
        Ok(file) => file,
        Err(error) => {
            return ProxyHealthProbe {
                issue: Some(format!(
                    "plugin MCP server command `{command}` resolved to {} but dogfood verify could not create proxy health output file: {error}",
                    path.display()
                )),
                ..ProxyHealthProbe::default()
            };
        }
    };

    let mut child_command = ProcessCommand::new(path);
    child_command
        .arg("--proxy-health")
        .env_remove("EXO_NO_REEXEC")
        .env_remove(DOGFOOD_ACTIVATION_ENV)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::from(output_file))
        .stderr(Stdio::null());
    configure_proxy_health_probe_process(&mut child_command);

    let mut child = match child_command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_file(&output_path);
            return ProxyHealthProbe {
                issue: Some(format!(
                    "plugin MCP server command `{command}` resolved to {} but failed to run proxy health probe: {error}",
                    path.display()
                )),
                ..ProxyHealthProbe::default()
            };
        }
    };

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= PROXY_HEALTH_TIMEOUT => {
                terminate_proxy_health_probe_process_group(child.id());
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&output_path);
                return ProxyHealthProbe {
                    issue: Some(format!(
                        "plugin MCP server command `{command}` resolved to {} but proxy health probe timed out after {}ms",
                        path.display(),
                        PROXY_HEALTH_TIMEOUT.as_millis()
                    )),
                    ..ProxyHealthProbe::default()
                };
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                terminate_proxy_health_probe_process_group(child.id());
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&output_path);
                return ProxyHealthProbe {
                    issue: Some(format!(
                        "plugin MCP server command `{command}` resolved to {} but failed while waiting for proxy health probe: {error}",
                        path.display()
                    )),
                    ..ProxyHealthProbe::default()
                };
            }
        }
    };
    terminate_proxy_health_probe_process_group(child.id());

    let stdout = match fs::read(&output_path) {
        Ok(stdout) => stdout,
        Err(error) => {
            let _ = fs::remove_file(&output_path);
            return ProxyHealthProbe {
                issue: Some(format!(
                    "plugin MCP server command `{command}` resolved to {} but failed to read proxy health output: {error}",
                    path.display()
                )),
                ..ProxyHealthProbe::default()
            };
        }
    };
    let _ = fs::remove_file(&output_path);

    if !status.success() {
        return ProxyHealthProbe {
            issue: Some(format!(
                "plugin MCP server command `{command}` resolved to {} but proxy health probe exited with {}",
                path.display(),
                status
            )),
            ..ProxyHealthProbe::default()
        };
    }
    let value = match serde_json::from_slice::<JsonValue>(&stdout) {
        Ok(value) => value,
        Err(error) => {
            return ProxyHealthProbe {
                issue: Some(format!(
                    "plugin MCP server command `{command}` resolved to {} but proxy health probe did not return valid JSON: {error}",
                    path.display()
                )),
                ..ProxyHealthProbe::default()
            };
        }
    };
    if value["kind"] != "exo-mcp.proxy-health"
        || value["worker_protocol_version"] != MCP_WORKER_PROTOCOL_VERSION
    {
        return ProxyHealthProbe {
            issue: Some(format!(
                "plugin MCP server command `{command}` resolved to {} but did not identify as a compatible exo-mcp proxy",
                path.display()
            )),
            ..ProxyHealthProbe::default()
        };
    }
    ProxyHealthProbe {
        issue: value
            .get("issue")
            .and_then(JsonValue::as_str)
            .map(str::to_string),
        proxy: probe_executable_identity(&value, "/status/proxy"),
        worker: probe_executable_identity(&value, "/status/worker/identity"),
        activation: value
            .pointer("/status/activation")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok()),
    }
}

fn configure_proxy_health_probe_process(command: &mut ProcessCommand) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    #[cfg(not(unix))]
    {
        let _ = command;
    }
}

fn terminate_proxy_health_probe_process_group(pid: u32) {
    #[cfg(unix)]
    {
        let Ok(pid) = i32::try_from(pid) else {
            return;
        };
        // A health probe may be a wrapper that leaves descendants holding probe
        // descriptors. Keep dogfood verification bounded by cleaning the group.
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(-pid),
            Some(nix::sys::signal::Signal::SIGTERM),
        );
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

fn probe_executable_identity(value: &JsonValue, pointer: &str) -> Option<ProbeExecutableIdentity> {
    let path = value
        .pointer(&format!("{pointer}/executable_path"))
        .and_then(JsonValue::as_str)
        .map(PathBuf::from)?;
    let path = path.canonicalize().unwrap_or(path);
    let identity = value.pointer(&format!("{pointer}/executable_identity"))?;
    let blake3 = identity
        .get("stable_hash")
        .and_then(JsonValue::as_str)
        .map(str::to_string)?;
    Some(ProbeExecutableIdentity {
        path,
        blake3,
        size_bytes: identity.get("len").and_then(JsonValue::as_u64),
        modified_unix_ms: identity
            .get("modified_unix_ms")
            .and_then(JsonValue::as_u64)
            .map(u128::from),
    })
}

fn find_command_in_path(command: &str) -> Option<PathBuf> {
    let binary = binary_name(command);
    let paths = std::env::var_os("PATH")?;
    let mut first_regular_file = None;
    for candidate in std::env::split_paths(&paths).map(|path| path.join(&binary)) {
        if !candidate.is_file() {
            continue;
        }
        if is_executable(&candidate) {
            return Some(candidate);
        }
        first_regular_file.get_or_insert(candidate);
    }
    first_regular_file
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    }

    #[cfg(not(unix))]
    {
        true
    }
}

fn binary_name(command: &str) -> String {
    if cfg!(windows) && Path::new(command).extension().is_none() {
        format!("{command}.exe")
    } else {
        command.to_string()
    }
}

fn is_exo_mcp_command(command: &str) -> bool {
    Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_exo_mcp_executable_name)
}

#[derive(Debug, Clone, Serialize)]
struct ProjectIdentity {
    id: String,
    git_common_dir: PathBuf,
    workspace_root: Option<PathBuf>,
    policy: &'static str,
    sidecar_key: Option<String>,
    sidecar_root: Option<PathBuf>,
}

impl ProjectIdentity {
    fn from_project(project: &Project) -> Self {
        Self {
            id: project.id.as_str().to_string(),
            git_common_dir: project.git_common_dir.clone(),
            workspace_root: project.workspace_root.clone(),
            policy: project.policy.as_str(),
            sidecar_key: project.sidecar_key.clone(),
            sidecar_root: project.sidecar_root.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ProjectPaths {
    state_root: PathBuf,
    db_path: PathBuf,
    runtime_dir: PathBuf,
    socket_path: PathBuf,
    pid_path: PathBuf,
    sidecar_manifest_path: Option<PathBuf>,
    sidecar_projection_dir: Option<PathBuf>,
}

impl ProjectPaths {
    fn from_project(project: &Project) -> Self {
        Self {
            state_root: project.state_root.clone(),
            db_path: project.db_path(),
            runtime_dir: project.runtime_dir(),
            socket_path: project.socket_path(),
            pid_path: project.pid_path(),
            sidecar_manifest_path: project.sidecar_manifest_path(),
            sidecar_projection_dir: project.sidecar_projection_dir(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct DaemonIdentity {
    runtime_dir: PathBuf,
    socket_path: PathBuf,
    pid_path: PathBuf,
    identity_path: Option<PathBuf>,
    pid: Option<u32>,
    pid_alive: Option<bool>,
    socket_exists: bool,
    socket_connectable: Option<bool>,
    state: DaemonStatusState,
    identity_exists: Option<bool>,
    identity_readable: Option<bool>,
    identity_matches_workspace: Option<bool>,
    identity_matches_executable: Option<bool>,
    required: bool,
    ok: bool,
    issue: Option<String>,
}

impl DaemonIdentity {
    fn from_project(root: &Path, project: &Project, required: bool) -> Self {
        let status = daemon_status_for_project(root, project);
        let runtime_dir = status
            .runtime_dir
            .clone()
            .unwrap_or_else(|| project.runtime_dir());
        let socket_path = status
            .socket_path
            .clone()
            .unwrap_or_else(|| project.socket_path());
        let pid_path = status
            .pid_path
            .clone()
            .unwrap_or_else(|| project.pid_path());
        let socket_exists = status.socket_exists.unwrap_or_else(|| socket_path.exists());
        let socket_connectable = if required {
            status.socket_connectable
        } else {
            None
        };
        let ok = !required || status.state == DaemonStatusState::RunningCurrent;
        let issue = if ok { None } else { status.issue.clone() };

        Self {
            runtime_dir,
            socket_path,
            pid_path,
            identity_path: status.identity_path,
            pid: status.pid,
            pid_alive: status.pid_alive,
            socket_exists,
            socket_connectable,
            state: status.state,
            identity_exists: status.identity_exists,
            identity_readable: status.identity_readable,
            identity_matches_workspace: status.identity_matches_workspace,
            identity_matches_executable: status.identity_matches_executable,
            required,
            ok,
            issue,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct RuntimeRestartReport {
    pid_path: PathBuf,
    socket_path: PathBuf,
    pid: Option<u32>,
    killed: bool,
    skipped_self: bool,
    socket_removed: bool,
    ensured: bool,
    ensure_state: Option<DaemonEnsureState>,
    connected: bool,
    spawned: bool,
    reused: bool,
    diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct McpRestartReport {
    scanned: usize,
    killed: Vec<McpProcessReport>,
    skipped_self: Vec<McpProcessReport>,
    preserved: Vec<McpProcessReport>,
}

#[derive(Debug, Clone, Serialize)]
struct McpProcessReport {
    pid: u32,
    command: String,
    cwd: Option<PathBuf>,
    killed: bool,
    action: &'static str,
    reason: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct SidecarIdentity {
    linked: bool,
    key: Option<String>,
    root: Option<PathBuf>,
    project_dir: Option<PathBuf>,
    manifest_path: Option<PathBuf>,
    projection_dir: Option<PathBuf>,
    auto_commit: bool,
    auto_push: &'static str,
    split_brain_errors: usize,
    split_brain_warnings: usize,
}

impl SidecarIdentity {
    fn from_project(project: &Project, split_brain: &SplitBrainReport) -> Self {
        Self {
            linked: matches!(project.policy, StatePolicy::Sidecar),
            key: project.sidecar_key.clone(),
            root: project.sidecar_root.clone(),
            project_dir: project.sidecar_project_dir(),
            manifest_path: project.sidecar_manifest_path(),
            projection_dir: project.sidecar_projection_dir(),
            auto_commit: project.sidecar_auto_commit,
            auto_push: project.sidecar_auto_push.as_str(),
            split_brain_errors: split_brain.errors,
            split_brain_warnings: split_brain.warnings,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct DogfoodPortability {
    ok: bool,
    warnings: usize,
    errors: usize,
    sidecar_git: Option<SidecarGitIdentity>,
}

impl DogfoodPortability {
    fn from_project(root: &Path, project: &Project) -> Self {
        if project.policy != StatePolicy::Sidecar {
            return Self {
                ok: true,
                warnings: 0,
                errors: 0,
                sidecar_git: None,
            };
        }

        let sidecar_git = SidecarGitIdentity::from_root(root);
        let errors = usize::from(sidecar_git.as_ref().is_some_and(|git| git.error));
        let warnings = usize::from(sidecar_git.as_ref().is_some_and(|git| git.warning));

        Self {
            ok: errors == 0,
            warnings,
            errors,
            sidecar_git,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct SidecarGitIdentity {
    status: SidecarRepoSyncStatus,
    last_commit: Option<String>,
    warning: bool,
    error: bool,
    severity: &'static str,
    issue: Option<String>,
}

impl SidecarGitIdentity {
    fn from_root(root: &Path) -> Option<Self> {
        let status = crate::command::sidecar::sidecar_repo_sync_status(root)?;
        let last_commit = git_output(&status.sidecar_root, &["rev-parse", "--short", "HEAD"]).ok();
        let issue = status.issue.clone();
        let no_remote = !status.has_remote;
        let dirty = !status.repo_clean || !status.foreign_checkpoint_debt.is_empty();
        let unpushed = status.has_remote && status.ahead.unwrap_or(0) > 0;
        let missing_upstream = status.has_remote
            && issue
                .as_deref()
                .is_some_and(|issue| issue.contains("not been pushed"));
        let error = dirty || unpushed || missing_upstream;
        let warning = !error && no_remote;
        let severity = if error {
            "error"
        } else if warning {
            "warning"
        } else {
            "ok"
        };

        Some(Self {
            status,
            last_commit,
            warning,
            error,
            severity,
            issue,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
struct SplitBrainReport {
    ok: bool,
    errors: usize,
    warnings: usize,
    canonical_db: DbSummary,
    candidates: Vec<SplitBrainCandidate>,
}

impl SplitBrainReport {
    fn scan(project: &Project, canonical_db_path: &Path) -> ExoResult<Self> {
        let canonical_db = DbSummary::inspect(canonical_db_path)?;
        let mut candidates = Vec::new();

        if matches!(project.policy, StatePolicy::Sidecar)
            && let Some(key) = project.sidecar_key.as_deref()
            && let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        {
            let legacy_state_root = home.join(".exo").join("sidecars").join(key);
            let legacy_db_path = legacy_state_root.join("cache").join("exo.db");
            if !same_path(&legacy_db_path, canonical_db_path) {
                candidates.push(SplitBrainCandidate::inspect(
                    "legacy-home-sidecar",
                    legacy_state_root,
                    legacy_db_path,
                    &canonical_db,
                )?);
            }
        }

        let errors = candidates
            .iter()
            .filter(|candidate| matches!(candidate.severity, SplitBrainSeverity::Error))
            .count();
        let warnings = candidates
            .iter()
            .filter(|candidate| matches!(candidate.severity, SplitBrainSeverity::Warning))
            .count();

        Ok(Self {
            ok: errors == 0,
            errors,
            warnings,
            canonical_db,
            candidates,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
struct SplitBrainCandidate {
    kind: &'static str,
    state_root: PathBuf,
    db_path: PathBuf,
    db: DbSummary,
    severity: SplitBrainSeverity,
    reason: String,
}

impl SplitBrainCandidate {
    fn inspect(
        kind: &'static str,
        state_root: PathBuf,
        db_path: PathBuf,
        canonical: &DbSummary,
    ) -> ExoResult<Self> {
        let db = DbSummary::inspect(&db_path)?;
        let legacy_has_more_rows = db.exo_rows > canonical.exo_rows;
        let legacy_is_newer = db.modified_unix_ms > canonical.modified_unix_ms;
        let severity = if db.exo_rows == 0 {
            SplitBrainSeverity::Ok
        } else if legacy_is_newer || legacy_has_more_rows {
            SplitBrainSeverity::Error
        } else {
            SplitBrainSeverity::Warning
        };
        let reason = match severity {
            SplitBrainSeverity::Ok => "legacy DB is absent or has no Exo rows".to_string(),
            SplitBrainSeverity::Warning => {
                "legacy DB contains Exo rows but is not newer than canonical DB".to_string()
            }
            SplitBrainSeverity::Error if legacy_is_newer && legacy_has_more_rows => {
                "legacy DB is newer and contains more Exo rows than the canonical DB".to_string()
            }
            SplitBrainSeverity::Error if legacy_is_newer => {
                "legacy DB contains newer Exo rows than the canonical DB".to_string()
            }
            SplitBrainSeverity::Error => {
                "legacy DB contains more Exo rows than the canonical DB".to_string()
            }
        };

        Ok(Self {
            kind,
            state_root,
            db_path,
            db,
            severity,
            reason,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SplitBrainSeverity {
    Ok,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
struct DbSummary {
    path: PathBuf,
    exists: bool,
    size_bytes: u64,
    modified_unix_ms: Option<u128>,
    exo_rows: u64,
    table_rows: Vec<TableRows>,
}

impl DbSummary {
    fn inspect(path: &Path) -> ExoResult<Self> {
        let metadata = fs::metadata(path).ok();
        let exists = metadata.is_some();
        let size_bytes = metadata.as_ref().map_or(0, fs::Metadata::len);
        let modified_unix_ms = metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(system_time_ms);
        let table_rows = if exists {
            count_exo_rows(path).unwrap_or_default()
        } else {
            Vec::new()
        };
        let exo_rows = table_rows
            .iter()
            .filter(|table| !table.ignored_for_split_brain)
            .map(|table| table.rows)
            .sum();

        Ok(Self {
            path: path.to_path_buf(),
            exists,
            size_bytes,
            modified_unix_ms,
            exo_rows,
            table_rows,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
struct TableRows {
    table: String,
    rows: u64,
    ignored_for_split_brain: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ReceiptComparison {
    present: bool,
    path: PathBuf,
    matches: bool,
    mismatches: Vec<ReceiptMismatch>,
}

#[derive(Debug, Clone, Serialize)]
struct ReceiptMismatch {
    field: &'static str,
    expected: String,
    actual: String,
}

#[derive(Debug, Clone, Serialize)]
struct RepairGuidance {
    required: bool,
    preview_command: Option<String>,
    note: String,
}

impl RepairGuidance {
    fn from_split_brain(split_brain: &SplitBrainReport) -> Self {
        if split_brain.errors == 0 {
            return Self {
                required: false,
                preview_command: None,
                note: "No newer legacy sidecar DB was detected.".to_string(),
            };
        }

        Self {
            required: true,
            preview_command: Some("exo dogfood repair".to_string()),
            note: "Repair is intentionally guided: inspect the divergent DB before replaying or migrating rows.".to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ReceiptExpected {
    binary: ReceiptBinary,
    extension: Option<ReceiptExtension>,
    plugin: Option<ReceiptPlugin>,
    project: ReceiptProject,
    paths: ReceiptPaths,
}

#[derive(Debug, Deserialize)]
struct ReceiptBinary {
    path: PathBuf,
    blake3: String,
}

#[derive(Debug, Deserialize)]
struct ReceiptExtension {
    build_stamp: String,
    bundle_sha256: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReceiptPlugin {
    blake3: String,
    proxy_binary: Option<ReceiptPluginProxyBinary>,
}

#[derive(Debug, Deserialize)]
struct ReceiptPluginProxyBinary {
    command: String,
    path: Option<PathBuf>,
    source: Option<String>,
    executable: bool,
    blake3: Option<String>,
    size_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ReceiptProject {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ReceiptPaths {
    state_root: PathBuf,
    db_path: PathBuf,
    runtime_dir: PathBuf,
    socket_path: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize)]
struct RepairPlan {
    ok: bool,
    canonical_db_path: PathBuf,
    candidates: Vec<RepairPlanCandidate>,
    totals: RepairTotals,
}

impl RepairPlan {
    fn build(paths: &ProjectPaths, split_brain: &SplitBrainReport) -> ExoResult<Self> {
        let candidates: Vec<RepairPlanCandidate> = split_brain
            .candidates
            .iter()
            .filter(|candidate| matches!(candidate.severity, SplitBrainSeverity::Error))
            .filter(|candidate| candidate.db.exists)
            .map(|candidate| RepairPlanCandidate::build(&paths.db_path, candidate))
            .collect::<ExoResult<Vec<_>>>()?;
        let totals = RepairTotals::from_candidates(&candidates);

        Ok(Self {
            ok: split_brain.errors == 0 || totals.has_replayable_rows(),
            canonical_db_path: paths.db_path.clone(),
            candidates,
            totals,
        })
    }
}

#[derive(Debug, Clone, Default, Serialize)]
struct RepairTotals {
    missing_goals: usize,
    missing_tasks: usize,
    missing_task_logs: usize,
    missing_task_verifications: usize,
    missing_inbox: usize,
    skipped_inbox: usize,
    ignored_agent_events: u64,
}

impl RepairTotals {
    fn from_candidates(candidates: &[RepairPlanCandidate]) -> Self {
        let mut totals = Self::default();
        for candidate in candidates {
            totals.missing_goals += candidate.missing_goals.len();
            totals.missing_tasks += candidate.missing_tasks.len();
            totals.missing_task_logs += candidate.missing_task_logs.len();
            totals.missing_task_verifications += candidate.missing_task_verifications.len();
            totals.missing_inbox += candidate.missing_inbox.len();
            totals.skipped_inbox += candidate.skipped_inbox.len();
            totals.ignored_agent_events += candidate.ignored_agent_events;
        }
        totals
    }

    const fn has_replayable_rows(&self) -> bool {
        self.missing_goals > 0
            || self.missing_tasks > 0
            || self.missing_task_logs > 0
            || self.missing_task_verifications > 0
            || self.missing_inbox > 0
    }
}

#[derive(Debug, Clone, Serialize)]
struct RepairPlanCandidate {
    kind: &'static str,
    db_path: PathBuf,
    missing_goals: Vec<RepairGoalRow>,
    missing_tasks: Vec<RepairTaskRow>,
    missing_task_logs: Vec<RepairTaskLogRow>,
    missing_task_verifications: Vec<RepairTaskVerificationRow>,
    missing_inbox: Vec<RepairInboxRow>,
    skipped_inbox: Vec<RepairSkippedInboxRow>,
    ignored_agent_events: u64,
}

impl RepairPlanCandidate {
    fn build(canonical_db_path: &Path, candidate: &SplitBrainCandidate) -> ExoResult<Self> {
        let conn = Connection::open_with_flags(canonical_db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("Failed to open DB {}", canonical_db_path.display()))?;
        attach_legacy(&conn, &candidate.db_path)?;

        let missing_goals = query_missing_goals(&conn)?;
        let missing_goal_ids = missing_goals
            .iter()
            .map(|row| row.text_id.clone())
            .collect::<HashSet<_>>();
        let missing_tasks = query_missing_tasks(&conn)?;
        let missing_task_ids = missing_tasks
            .iter()
            .map(|row| row.text_id.clone())
            .collect::<HashSet<_>>();
        let missing_task_logs = query_missing_task_logs(&conn)?;
        let missing_task_verifications = query_missing_task_verifications(&conn)?;
        let (missing_inbox, skipped_inbox) =
            query_missing_inbox(&conn, &missing_goal_ids, &missing_task_ids)?;
        let ignored_agent_events = count_table_rows(&conn, "legacy.agent_events")?;

        Ok(Self {
            kind: candidate.kind,
            db_path: candidate.db_path.clone(),
            missing_goals,
            missing_tasks,
            missing_task_logs,
            missing_task_verifications,
            missing_inbox,
            skipped_inbox,
            ignored_agent_events,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
struct RepairGoalRow {
    text_id: String,
    label: String,
    status: String,
    phase_text_id: String,
    kind: Option<String>,
    rfc: Option<String>,
    target_stage: Option<i64>,
    started_at: Option<String>,
    description: Option<String>,
    completion_log: Option<String>,
    slug: Option<String>,
    sort_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RepairTaskRow {
    text_id: String,
    title: String,
    status: String,
    goal_text_id: String,
    completed_at: Option<String>,
    completion_log: Option<String>,
    slug: Option<String>,
    sort_key: Option<String>,
    notes: Option<String>,
    started_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RepairTaskLogRow {
    task_text_id: String,
    kind: String,
    message: String,
    created_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct RepairTaskVerificationRow {
    task_text_id: String,
    kind: String,
    command: Option<String>,
    result: String,
    details: Option<String>,
    created_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct RepairInboxRow {
    text_id: String,
    created_at: String,
    updated_at: Option<String>,
    status: String,
    entity_type: String,
    entity_id: Option<String>,
    source: String,
    intent: String,
    priority: String,
    confidence: Option<String>,
    subject: String,
    body: String,
    resolution: Option<String>,
    agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RepairSkippedInboxRow {
    text_id: String,
    entity_type: String,
    entity_id: Option<String>,
    reason: String,
}

#[derive(Debug, Clone, Serialize)]
struct RepairApplyReport {
    ok: bool,
    backup_path: Option<PathBuf>,
    inserted_goals: usize,
    inserted_tasks: usize,
    inserted_task_logs: usize,
    inserted_task_verifications: usize,
    inserted_inbox: usize,
    ignored_agent_events: u64,
    sidecar_auto_persist: Option<SidecarAutoPersistReport>,
}

fn query_missing_goals(conn: &Connection) -> ExoResult<Vec<RepairGoalRow>> {
    let mut stmt = conn.prepare(
        "SELECT g.text_id, g.label, g.status, p.text_id, g.kind, g.rfc, g.target_stage,
                g.started_at, g.description, g.completion_log, g.slug, g.sort_key
         FROM legacy.goals_data g
         JOIN legacy.phases_data p ON p.id = g.phase_id
         WHERE NOT EXISTS (SELECT 1 FROM main.goals_data cg WHERE cg.text_id = g.text_id)
           AND EXISTS (SELECT 1 FROM main.phases_data cp WHERE cp.text_id = p.text_id)
         ORDER BY g.id",
    )?;
    stmt.query_map([], |row| {
        Ok(RepairGoalRow {
            text_id: row.get(0)?,
            label: row.get(1)?,
            status: row.get(2)?,
            phase_text_id: row.get(3)?,
            kind: row.get(4)?,
            rfc: row.get(5)?,
            target_stage: row.get(6)?,
            started_at: row.get(7)?,
            description: row.get(8)?,
            completion_log: row.get(9)?,
            slug: row.get(10)?,
            sort_key: row.get(11)?,
        })
    })?
    .collect::<Result<Vec<_>, _>>()
    .context("Failed to query missing goals")
}

fn query_missing_tasks(conn: &Connection) -> ExoResult<Vec<RepairTaskRow>> {
    let mut stmt = conn.prepare(
        "SELECT t.text_id, t.title, t.status, g.text_id, t.completed_at, t.completion_log,
                t.slug, t.sort_key, t.notes, t.started_at
         FROM legacy.tasks_data t
         JOIN legacy.goals_data g ON g.id = t.goal_id
         JOIN legacy.phases_data p ON p.id = g.phase_id
         WHERE NOT EXISTS (SELECT 1 FROM main.tasks_data ct WHERE ct.text_id = t.text_id)
           AND (
             EXISTS (SELECT 1 FROM main.goals_data cg WHERE cg.text_id = g.text_id)
             OR EXISTS (SELECT 1 FROM main.phases_data cp WHERE cp.text_id = p.text_id)
           )
         ORDER BY t.id",
    )?;
    stmt.query_map([], |row| {
        Ok(RepairTaskRow {
            text_id: row.get(0)?,
            title: row.get(1)?,
            status: row.get(2)?,
            goal_text_id: row.get(3)?,
            completed_at: row.get(4)?,
            completion_log: row.get(5)?,
            slug: row.get(6)?,
            sort_key: row.get(7)?,
            notes: row.get(8)?,
            started_at: row.get(9)?,
        })
    })?
    .collect::<Result<Vec<_>, _>>()
    .context("Failed to query missing tasks")
}

fn query_missing_task_logs(conn: &Connection) -> ExoResult<Vec<RepairTaskLogRow>> {
    let mut stmt = conn.prepare(
        "SELECT t.text_id, l.kind, l.message, l.created_at
         FROM legacy.task_logs l
         JOIN legacy.tasks_data t ON t.id = l.task_id
         JOIN legacy.goals_data g ON g.id = t.goal_id
         JOIN legacy.phases_data p ON p.id = g.phase_id
         WHERE NOT EXISTS (
             SELECT 1
             FROM main.task_logs cl
             JOIN main.tasks_data ct ON ct.id = cl.task_id
             WHERE ct.text_id = t.text_id
               AND cl.kind = l.kind
               AND cl.message = l.message
               AND cl.created_at = l.created_at
           )
           AND (
             EXISTS (SELECT 1 FROM main.tasks_data ct WHERE ct.text_id = t.text_id)
             OR EXISTS (SELECT 1 FROM main.goals_data cg WHERE cg.text_id = g.text_id)
             OR EXISTS (SELECT 1 FROM main.phases_data cp WHERE cp.text_id = p.text_id)
           )
         ORDER BY l.id",
    )?;
    stmt.query_map([], |row| {
        Ok(RepairTaskLogRow {
            task_text_id: row.get(0)?,
            kind: row.get(1)?,
            message: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?
    .collect::<Result<Vec<_>, _>>()
    .context("Failed to query missing task logs")
}

fn query_missing_task_verifications(
    conn: &Connection,
) -> ExoResult<Vec<RepairTaskVerificationRow>> {
    let mut stmt = conn.prepare(
        "SELECT t.text_id, v.kind, v.command, v.result, v.details, v.created_at
         FROM legacy.task_verifications v
         JOIN legacy.tasks_data t ON t.id = v.task_id
         JOIN legacy.goals_data g ON g.id = t.goal_id
         JOIN legacy.phases_data p ON p.id = g.phase_id
         WHERE NOT EXISTS (
             SELECT 1
             FROM main.task_verifications cv
             JOIN main.tasks_data ct ON ct.id = cv.task_id
             WHERE ct.text_id = t.text_id
               AND cv.kind = v.kind
               AND ((cv.command IS NULL AND v.command IS NULL) OR cv.command = v.command)
               AND cv.result = v.result
               AND ((cv.details IS NULL AND v.details IS NULL) OR cv.details = v.details)
               AND cv.created_at = v.created_at
           )
           AND (
             EXISTS (SELECT 1 FROM main.tasks_data ct WHERE ct.text_id = t.text_id)
             OR EXISTS (SELECT 1 FROM main.goals_data cg WHERE cg.text_id = g.text_id)
             OR EXISTS (SELECT 1 FROM main.phases_data cp WHERE cp.text_id = p.text_id)
           )
         ORDER BY v.id",
    )?;
    stmt.query_map([], |row| {
        Ok(RepairTaskVerificationRow {
            task_text_id: row.get(0)?,
            kind: row.get(1)?,
            command: row.get(2)?,
            result: row.get(3)?,
            details: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?
    .collect::<Result<Vec<_>, _>>()
    .context("Failed to query missing task verifications")
}

fn query_missing_inbox(
    conn: &Connection,
    missing_goal_ids: &HashSet<String>,
    missing_task_ids: &HashSet<String>,
) -> ExoResult<(Vec<RepairInboxRow>, Vec<RepairSkippedInboxRow>)> {
    let mut stmt = conn.prepare(
        "SELECT text_id, created_at, updated_at, status, entity_type, entity_id, source, intent,
                priority, confidence, subject, body, resolution, agent_id
         FROM legacy.inbox_data i
         WHERE NOT EXISTS (SELECT 1 FROM main.inbox_data ci WHERE ci.text_id = i.text_id)
         ORDER BY id",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(RepairInboxRow {
                text_id: row.get(0)?,
                created_at: row.get(1)?,
                updated_at: row.get(2)?,
                status: row.get(3)?,
                entity_type: row.get(4)?,
                entity_id: row.get(5)?,
                source: row.get(6)?,
                intent: row.get(7)?,
                priority: row.get(8)?,
                confidence: row.get(9)?,
                subject: row.get(10)?,
                body: row.get(11)?,
                resolution: row.get(12)?,
                agent_id: row.get(13)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to query missing inbox rows")?;

    let mut replayable = Vec::new();
    let mut skipped = Vec::new();
    for row in rows {
        if inbox_entity_replayable(conn, &row, missing_goal_ids, missing_task_ids)? {
            replayable.push(row);
        } else {
            skipped.push(RepairSkippedInboxRow {
                text_id: row.text_id,
                entity_type: row.entity_type,
                entity_id: row.entity_id,
                reason: "referenced entity is not present or replayable".to_string(),
            });
        }
    }

    Ok((replayable, skipped))
}

fn inbox_entity_replayable(
    conn: &Connection,
    row: &RepairInboxRow,
    missing_goal_ids: &HashSet<String>,
    missing_task_ids: &HashSet<String>,
) -> ExoResult<bool> {
    let Some(entity_id) = row.entity_id.as_deref() else {
        return Ok(row.entity_type == "project");
    };
    match row.entity_type.as_str() {
        "project" => Ok(true),
        "goal" => {
            Ok(missing_goal_ids.contains(entity_id)
                || text_id_exists(conn, "goals_data", entity_id)?)
        }
        "task" => {
            Ok(missing_task_ids.contains(entity_id)
                || text_id_exists(conn, "tasks_data", entity_id)?)
        }
        "phase" => text_id_exists(conn, "phases_data", entity_id),
        "epoch" => text_id_exists(conn, "epochs_data", entity_id),
        "rfc" => text_id_exists(conn, "rfcs_data", entity_id),
        _ => Ok(false),
    }
}

fn text_id_exists(conn: &Connection, table: &str, text_id: &str) -> ExoResult<bool> {
    let sql = format!("SELECT 1 FROM main.{table} WHERE text_id = ?1");
    Ok(conn
        .query_row(&sql, [text_id], |_| Ok(()))
        .optional()?
        .is_some())
}

fn count_table_rows(conn: &Connection, table: &str) -> ExoResult<u64> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM legacy.sqlite_master WHERE type = 'table' AND name = ?1",
            [table.strip_prefix("legacy.").unwrap_or(table)],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Ok(0);
    }
    let count: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })?;
    Ok(u64::try_from(count).unwrap_or_default())
}

fn apply_repair_plan(
    root: &Path,
    project: &Project,
    paths: &ProjectPaths,
    plan: &RepairPlan,
) -> ExoResult<RepairApplyReport> {
    if !plan.totals.has_replayable_rows() {
        bail!("Dogfood repair has no replayable rows. Review `exo dogfood repair` output first.");
    }
    if crate::command::sidecar::sidecar_write_ownership_applies_to_project(project) {
        crate::command::sidecar::ensure_sidecar_write_ownership_for_project(project)?;
    }

    let backup_path = backup_canonical_db(paths)?;
    let mut conn = Connection::open(&paths.db_path)
        .with_context(|| format!("Failed to open DB {}", paths.db_path.display()))?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;

    let tx = conn.transaction()?;
    let mut inserted_goals = 0;
    let mut inserted_tasks = 0;
    let mut inserted_task_logs = 0;
    let mut inserted_task_verifications = 0;
    let mut inserted_inbox = 0;
    let mut ignored_agent_events = 0;

    for candidate in &plan.candidates {
        ignored_agent_events += candidate.ignored_agent_events;
        for row in &candidate.missing_goals {
            let phase_id: i64 = tx.query_row(
                "SELECT id FROM phases_data WHERE text_id = ?1",
                [&row.phase_text_id],
                |row| row.get(0),
            )?;
            inserted_goals += tx.execute(
                "INSERT OR IGNORE INTO goals_data
                 (text_id, label, status, phase_id, kind, rfc, target_stage, started_at,
                  description, completion_log, slug, sort_key)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    row.text_id,
                    row.label,
                    row.status,
                    phase_id,
                    row.kind,
                    row.rfc,
                    row.target_stage,
                    row.started_at,
                    row.description,
                    row.completion_log,
                    row.slug,
                    row.sort_key
                ],
            )?;
        }

        for row in &candidate.missing_tasks {
            let goal_id: i64 = tx.query_row(
                "SELECT id FROM goals_data WHERE text_id = ?1",
                [&row.goal_text_id],
                |row| row.get(0),
            )?;
            inserted_tasks += tx.execute(
                "INSERT OR IGNORE INTO tasks_data
                 (text_id, title, status, goal_id, completed_at, completion_log, slug, sort_key,
                  notes, started_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    row.text_id,
                    row.title,
                    row.status,
                    goal_id,
                    row.completed_at,
                    row.completion_log,
                    row.slug,
                    row.sort_key,
                    row.notes,
                    row.started_at
                ],
            )?;
        }

        for row in &candidate.missing_task_logs {
            let task_id: i64 = tx.query_row(
                "SELECT id FROM tasks_data WHERE text_id = ?1",
                [&row.task_text_id],
                |row| row.get(0),
            )?;
            inserted_task_logs += tx.execute(
                "INSERT INTO task_logs (task_id, kind, message, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![task_id, row.kind, row.message, row.created_at],
            )?;
        }

        for row in &candidate.missing_task_verifications {
            let task_id: i64 = tx.query_row(
                "SELECT id FROM tasks_data WHERE text_id = ?1",
                [&row.task_text_id],
                |row| row.get(0),
            )?;
            inserted_task_verifications += tx.execute(
                "INSERT INTO task_verifications (task_id, kind, command, result, details, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    task_id,
                    row.kind,
                    row.command,
                    row.result,
                    row.details,
                    row.created_at
                ],
            )?;
        }

        for row in &candidate.missing_inbox {
            inserted_inbox += tx.execute(
                "INSERT OR IGNORE INTO inbox_data
                 (text_id, created_at, updated_at, status, entity_type, entity_id, source, intent,
                  priority, confidence, subject, body, resolution, agent_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    row.text_id,
                    row.created_at,
                    row.updated_at,
                    row.status,
                    row.entity_type,
                    row.entity_id,
                    row.source,
                    row.intent,
                    row.priority,
                    row.confidence,
                    row.subject,
                    row.body,
                    row.resolution,
                    row.agent_id
                ],
            )?;
        }
    }

    tx.commit()?;

    let sidecar_auto_persist = crate::post_write::with_sidecar_runtime_lock(Some(project), || {
        crate::command::sidecar::checkpoint_after_successful_mutation_with_project(project).map_err(
            |error| {
                crate::post_write::sidecar_checkpoint_failure(
                    project,
                    "dogfood",
                    "repair",
                    Effect::Exec,
                    error,
                )
            },
        )
    })?;
    let _ = root;

    Ok(RepairApplyReport {
        ok: true,
        backup_path: Some(backup_path),
        inserted_goals,
        inserted_tasks,
        inserted_task_logs,
        inserted_task_verifications,
        inserted_inbox,
        ignored_agent_events,
        sidecar_auto_persist,
    })
}

fn backup_canonical_db(paths: &ProjectPaths) -> ExoResult<PathBuf> {
    let backup_dir = paths.runtime_dir.join("repair-backups");
    fs::create_dir_all(&backup_dir).with_context(|| {
        format!(
            "Failed to create repair backup dir {}",
            backup_dir.display()
        )
    })?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let backup_path = backup_dir.join(format!("exo-{stamp}.db"));
    fs::copy(&paths.db_path, &backup_path).with_context(|| {
        format!(
            "Failed to back up canonical DB {} to {}",
            paths.db_path.display(),
            backup_path.display()
        )
    })?;
    Ok(backup_path)
}

fn attach_legacy(conn: &Connection, db_path: &Path) -> ExoResult<()> {
    let db_path = db_path
        .to_str()
        .ok_or_else(|| anyhow!("legacy DB path is not UTF-8: {}", db_path.display()))?;
    conn.execute("ATTACH DATABASE ?1 AS legacy", [db_path])
        .context("Failed to attach legacy sidecar DB")?;
    Ok(())
}

fn default_plugin_identity(
    root: &Path,
    current_binary: &BinaryIdentity,
) -> ExoResult<Option<PluginIdentity>> {
    if let Some(pinned_config) =
        DogfoodActivation::pinned_mcp_config_from_environment().map_err(|error| anyhow!(error))?
    {
        let plugin_dir = pinned_config.parent().ok_or_else(|| {
            anyhow!(
                "Pinned Codex plugin config has no plugin directory: {}",
                pinned_config.display()
            )
        })?;
        return PluginIdentity::from_dir(root, plugin_dir, current_binary).map(Some);
    }
    let plugin_dir = root.join("plugins/exo");
    if plugin_dir.is_dir() {
        return PluginIdentity::from_dir(root, &plugin_dir, current_binary).map(Some);
    }
    Ok(None)
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> ExoResult<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("Failed to read entry in {}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("Failed to inspect {}", path.display()))?;
        if file_type.is_dir() {
            collect_files(&path, files)?;
        } else if file_type.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn ensure_daemon_runtime(root: &Path, project: &Project) -> ExoResult<RuntimeRestartReport> {
    let rt = tokio::runtime::Runtime::new().context("Failed to create daemon ensure runtime")?;
    let report = rt
        .block_on(ensure_daemon_with_report(root))
        .map(DaemonEnsureOutcome::into_report)
        .context("Failed to ensure Exo daemon runtime")?;
    let pid_path = project.pid_path();
    let socket_path = project.socket_path();

    Ok(RuntimeRestartReport {
        pid_path,
        socket_path,
        pid: report.pid,
        killed: false,
        skipped_self: false,
        socket_removed: false,
        ensured: true,
        ensure_state: Some(report.state),
        connected: report.connected,
        spawned: report.spawned,
        reused: report.reused,
        diagnostics: report.diagnostics,
    })
}

#[cfg(windows)]
fn inspect_workspace_mcp_servers(root: &Path) -> ExoResult<McpRestartReport> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let current_pid = std::process::id();
    let target_exe = debug_binary_path(&root, "exo");
    let target_proxy_exe = debug_binary_path(&root, "exo-mcp");
    let output = ProcessCommand::new("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,CommandLine | ConvertTo-Json -Compress",
        ])
        .output()
        .context("Failed to scan Windows processes")?;
    if !output.status.success() {
        bail!(
            "Failed to scan Windows processes: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let value: JsonValue = serde_json::from_slice(&output.stdout)
        .context("Failed to parse Windows process scan output")?;
    let processes: Vec<WindowsProcess> = match value {
        JsonValue::Array(items) => items
            .into_iter()
            .filter_map(|item| serde_json::from_value(item).ok())
            .collect(),
        JsonValue::Object(_) => vec![
            serde_json::from_value(value).context("Failed to parse Windows process scan row")?,
        ],
        _ => Vec::new(),
    };
    let current_parent_pid = processes
        .iter()
        .find(|process| process.process_id == Some(current_pid))
        .and_then(|process| process.parent_process_id);
    let mut scanned = 0;
    let killed = Vec::new();
    let mut skipped_self = Vec::new();
    let mut preserved = Vec::new();

    for process in processes {
        let Some(pid) = process.process_id else {
            continue;
        };
        let command = process.command_line.unwrap_or_default();
        if !is_mcp_runtime_command(&command) {
            continue;
        }
        scanned += 1;
        let cwd = process_cwd(pid);
        match mcp_process_decision(
            pid,
            &command,
            cwd.as_deref(),
            current_pid,
            current_parent_pid,
            &target_exe,
            &target_proxy_exe,
            &root,
        ) {
            McpProcessDecision::Ignore => {}
            McpProcessDecision::SkipSelf => skipped_self.push(McpProcessReport {
                pid,
                command,
                cwd,
                killed: false,
                action: "preserved",
                reason: "current_command_transport",
            }),
            McpProcessDecision::PreserveDurableProxy => preserved.push(McpProcessReport {
                pid,
                command,
                cwd,
                killed: false,
                action: "preserved",
                reason: "durable_proxy_transport",
            }),
            McpProcessDecision::PreserveLegacyServer => preserved.push(McpProcessReport {
                pid,
                command,
                cwd,
                killed: false,
                action: "preserved",
                reason: "legacy_mcp_server_transport",
            }),
        }
    }

    Ok(McpRestartReport {
        scanned,
        killed,
        skipped_self,
        preserved,
    })
}

#[cfg(windows)]
#[derive(Debug, Deserialize)]
struct WindowsProcess {
    #[serde(rename = "ProcessId")]
    process_id: Option<u32>,
    #[serde(rename = "ParentProcessId")]
    parent_process_id: Option<u32>,
    #[serde(rename = "CommandLine")]
    command_line: Option<String>,
}

#[cfg(not(windows))]
fn inspect_workspace_mcp_servers(root: &Path) -> ExoResult<McpRestartReport> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let current_pid = std::process::id();
    let target_exe = debug_binary_path(&root, "exo");
    let target_proxy_exe = debug_binary_path(&root, "exo-mcp");
    let output = std::process::Command::new("ps")
        .args(["-axo", "pid=,ppid=,command="])
        .output()
        .context("Failed to scan processes with ps")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let current_parent_pid = stdout
        .lines()
        .filter_map(parse_ps_line)
        .find_map(|process| (process.pid == current_pid).then_some(process.parent_pid));
    let mut scanned = 0;
    let killed = Vec::new();
    let mut skipped_self = Vec::new();
    let mut preserved = Vec::new();

    for line in stdout.lines() {
        let Some(process) = parse_ps_line(line) else {
            continue;
        };
        let pid = process.pid;
        let command = process.command;
        if !is_mcp_runtime_command(command) {
            continue;
        }
        scanned += 1;

        let cwd = process_cwd(pid);
        match mcp_process_decision(
            pid,
            command,
            cwd.as_deref(),
            current_pid,
            current_parent_pid,
            &target_exe,
            &target_proxy_exe,
            &root,
        ) {
            McpProcessDecision::Ignore => {}
            McpProcessDecision::SkipSelf => skipped_self.push(McpProcessReport {
                pid,
                command: command.to_string(),
                cwd,
                killed: false,
                action: "preserved",
                reason: "current_command_transport",
            }),
            McpProcessDecision::PreserveDurableProxy => preserved.push(McpProcessReport {
                pid,
                command: command.to_string(),
                cwd,
                killed: false,
                action: "preserved",
                reason: "durable_proxy_transport",
            }),
            McpProcessDecision::PreserveLegacyServer => preserved.push(McpProcessReport {
                pid,
                command: command.to_string(),
                cwd,
                killed: false,
                action: "preserved",
                reason: "legacy_mcp_server_transport",
            }),
        }
    }

    Ok(McpRestartReport {
        scanned,
        killed,
        skipped_self,
        preserved,
    })
}

fn debug_binary_path(root: &Path, name: &str) -> PathBuf {
    root.join("target")
        .join("debug")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

#[cfg(not(windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PsProcess<'a> {
    pid: u32,
    parent_pid: u32,
    command: &'a str,
}

#[cfg(not(windows))]
fn parse_ps_line(line: &str) -> Option<PsProcess<'_>> {
    let trimmed = line.trim_start();
    let pid_split = trimmed.find(char::is_whitespace)?;
    let (pid, rest) = trimmed.split_at(pid_split);
    let rest = rest.trim_start();
    let parent_split = rest.find(char::is_whitespace)?;
    let (parent_pid, command) = rest.split_at(parent_split);
    let command = command.trim_start();
    (!command.is_empty()).then_some(PsProcess {
        pid: pid.parse().ok()?,
        parent_pid: parent_pid.parse().ok()?,
        command,
    })
}

fn is_mcp_runtime_command(command: &str) -> bool {
    command.contains("mcp serve")
        || command_executable_name(command).is_some_and(is_exo_mcp_executable_name)
}

fn command_executable_name(command: &str) -> Option<&str> {
    let program = command.split_whitespace().next()?.trim_matches('"');
    Path::new(program).file_name()?.to_str()
}

fn is_exo_mcp_executable_name(name: &str) -> bool {
    name == "exo-mcp" || name.eq_ignore_ascii_case("exo-mcp.exe")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpProcessDecision {
    Ignore,
    SkipSelf,
    PreserveDurableProxy,
    PreserveLegacyServer,
}

fn mcp_process_decision(
    pid: u32,
    command: &str,
    cwd: Option<&Path>,
    current_pid: u32,
    current_parent_pid: Option<u32>,
    target_exe: &Path,
    target_proxy_exe: &Path,
    root: &Path,
) -> McpProcessDecision {
    if !is_mcp_runtime_command(command) {
        return McpProcessDecision::Ignore;
    }

    if pid == current_pid || Some(pid) == current_parent_pid {
        return McpProcessDecision::SkipSelf;
    }

    let command_matches_target = [target_exe, target_proxy_exe]
        .iter()
        .any(|target| command.contains(&target.display().to_string()));
    let cwd_matches_root = cwd.is_some_and(|cwd| same_path(cwd, root));

    if !(command_matches_target || cwd_matches_root) {
        return McpProcessDecision::Ignore;
    }

    if command_executable_name(command).is_some_and(is_exo_mcp_executable_name) {
        McpProcessDecision::PreserveDurableProxy
    } else {
        McpProcessDecision::PreserveLegacyServer
    }
}

#[cfg(not(windows))]
fn process_cwd(pid: u32) -> Option<PathBuf> {
    let output = std::process::Command::new("lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix('n').map(PathBuf::from))
}

#[cfg(windows)]
fn process_cwd(_pid: u32) -> Option<PathBuf> {
    None
}

fn git_output(root: &Path, args: &[&str]) -> ExoResult<String> {
    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .with_context(|| format!("Failed to run git in {}", root.display()))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn file_blake3(path: &Path) -> ExoResult<String> {
    let mut file =
        fs::File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn system_time_ms(time: std::time::SystemTime) -> Option<u128> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis())
}

fn count_exo_rows(path: &Path) -> ExoResult<Vec<TableRows>> {
    const TABLES: &[(&str, bool)] = &[
        ("epochs_data", false),
        ("phases_data", false),
        ("goals_data", false),
        ("tasks_data", false),
        ("inbox_data", false),
        ("rfcs_data", false),
        ("agent_events", true),
    ];

    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("Failed to open DB {}", path.display()))?;
    let mut rows = Vec::new();

    for &(table, ignored_for_split_brain) in TABLES {
        let exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_none() {
            continue;
        }

        let count: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })?;
        rows.push(TableRows {
            table: table.to_string(),
            rows: u64::try_from(count).unwrap_or_default(),
            ignored_for_split_brain,
        });
    }

    Ok(rows)
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    left == right
}

fn compare_path(
    field: &'static str,
    expected: Option<&PathBuf>,
    actual: &Path,
    mismatches: &mut Vec<ReceiptMismatch>,
) {
    let Some(expected) = expected else {
        return;
    };
    if !same_path(expected, actual) {
        mismatches.push(ReceiptMismatch {
            field,
            expected: expected.display().to_string(),
            actual: actual.display().to_string(),
        });
    }
}

fn compare_optional_path(
    field: &'static str,
    expected: Option<&PathBuf>,
    actual: Option<&PathBuf>,
    mismatches: &mut Vec<ReceiptMismatch>,
) {
    let Some(expected) = expected else {
        return;
    };
    match actual {
        Some(actual) if same_path(expected, actual) => {}
        Some(actual) => mismatches.push(ReceiptMismatch {
            field,
            expected: expected.display().to_string(),
            actual: actual.display().to_string(),
        }),
        None => mismatches.push(ReceiptMismatch {
            field,
            expected: expected.display().to_string(),
            actual: "<missing>".to_string(),
        }),
    }
}

fn compare_string(
    field: &'static str,
    expected: Option<&str>,
    actual: &str,
    mismatches: &mut Vec<ReceiptMismatch>,
) {
    let Some(expected) = expected else {
        return;
    };
    if expected != actual {
        mismatches.push(ReceiptMismatch {
            field,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }
}

fn compare_optional_string(
    field: &'static str,
    expected: Option<&str>,
    actual: Option<&str>,
    mismatches: &mut Vec<ReceiptMismatch>,
) {
    let Some(expected) = expected else {
        return;
    };
    match actual {
        Some(actual) if actual == expected => {}
        Some(actual) => mismatches.push(ReceiptMismatch {
            field,
            expected: expected.to_string(),
            actual: actual.to_string(),
        }),
        None => mismatches.push(ReceiptMismatch {
            field,
            expected: expected.to_string(),
            actual: "<missing>".to_string(),
        }),
    }
}

fn compare_bool(
    field: &'static str,
    expected: Option<bool>,
    actual: bool,
    mismatches: &mut Vec<ReceiptMismatch>,
) {
    let Some(expected) = expected else {
        return;
    };
    if expected != actual {
        mismatches.push(ReceiptMismatch {
            field,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }
}

fn compare_optional_u64(
    field: &'static str,
    expected: Option<u64>,
    actual: Option<u64>,
    mismatches: &mut Vec<ReceiptMismatch>,
) {
    let Some(expected) = expected else {
        return;
    };
    match actual {
        Some(actual) if actual == expected => {}
        Some(actual) => mismatches.push(ReceiptMismatch {
            field,
            expected: expected.to_string(),
            actual: actual.to_string(),
        }),
        None => mismatches.push(ReceiptMismatch {
            field,
            expected: expected.to_string(),
            actual: "<missing>".to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn proxy_health_probe_does_not_wait_for_background_stdout_holder() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let proxy = temp.path().join("exo-mcp");
        let payload = serde_json::json!({
            "kind": "exo-mcp.proxy-health",
            "worker_protocol_version": MCP_WORKER_PROTOCOL_VERSION,
            "status": {},
        });
        let mut file = fs::File::create(&proxy).expect("create fake proxy");
        writeln!(
            file,
            "#!/bin/sh\nif [ \"$1\" = \"--proxy-health\" ]; then\n  (sleep 60) &\n  printf '%s\\n' '{}'\n  exit 0\nfi\nexit 0",
            payload
        )
        .expect("write fake proxy");
        drop(file);
        let mut permissions = fs::metadata(&proxy).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&proxy, permissions).expect("chmod fake proxy");

        let started = Instant::now();
        let probe = proxy_health_probe("exo-mcp", &proxy, &BTreeMap::new());

        assert_eq!(probe.issue, None);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "proxy health probe waited for inherited background stdout"
        );
    }

    #[test]
    fn dogfood_verify_metadata() {
        let cmd = DogfoodVerify::default();
        assert_eq!(cmd.namespace(), "dogfood");
        assert_eq!(cmd.operation(), "verify");
        assert_eq!(cmd.effect(), Effect::Pure);
    }

    #[test]
    fn dogfood_repair_metadata() {
        let cmd = DogfoodRepair::new(false);
        assert_eq!(cmd.namespace(), "dogfood");
        assert_eq!(cmd.operation(), "repair");
        assert_eq!(cmd.effect(), Effect::Pure);

        let apply = DogfoodRepair::new(true);
        assert_eq!(apply.namespace(), "dogfood");
        assert_eq!(apply.operation(), "repair");
        assert_eq!(apply.effect(), Effect::Exec);
    }

    #[test]
    fn restart_mcp_scan_skips_current_mcp_process() {
        let root = Path::new("/workspace/exo2");
        let target_exe = root.join("target/debug/exo");
        let target_proxy_exe = root.join("target/debug/exo-mcp");
        let command = format!("{} mcp serve", target_exe.display());

        let decision = mcp_process_decision(
            42,
            &command,
            Some(root),
            42,
            None,
            &target_exe,
            &target_proxy_exe,
            root,
        );

        assert_eq!(decision, McpProcessDecision::SkipSelf);
    }

    #[test]
    fn restart_mcp_scan_preserves_other_workspace_mcp_process() {
        let root = Path::new("/workspace/exo2");
        let target_exe = root.join("target/debug/exo");
        let target_proxy_exe = root.join("target/debug/exo-mcp");
        let command = format!("{} mcp serve", target_exe.display());

        let decision = mcp_process_decision(
            41,
            &command,
            Some(root),
            42,
            None,
            &target_exe,
            &target_proxy_exe,
            root,
        );

        assert_eq!(decision, McpProcessDecision::PreserveLegacyServer);
    }

    #[test]
    fn restart_mcp_scan_ignores_same_binary_in_other_workspace() {
        let root = Path::new("/workspace/exo2");
        let other_root = Path::new("/workspace/other");
        let target_exe = root.join("target/debug/exo");
        let target_proxy_exe = root.join("target/debug/exo-mcp");
        let command = format!("{} mcp serve", std::env::current_exe().unwrap().display());

        let decision = mcp_process_decision(
            41,
            &command,
            Some(other_root),
            42,
            None,
            &target_exe,
            &target_proxy_exe,
            root,
        );

        assert_eq!(decision, McpProcessDecision::Ignore);
    }

    #[test]
    fn restart_mcp_scan_preserves_workspace_exo_mcp_proxy_process() {
        let root = Path::new("/workspace/exo2");
        let target_exe = root.join("target/debug/exo");
        let target_proxy_exe = root.join("target/debug/exo-mcp");
        let command = target_proxy_exe.display().to_string();

        let decision = mcp_process_decision(
            41,
            &command,
            Some(root),
            42,
            Some(40),
            &target_exe,
            &target_proxy_exe,
            root,
        );

        assert_eq!(decision, McpProcessDecision::PreserveDurableProxy);
    }

    #[test]
    fn restart_mcp_scan_skips_current_exo_mcp_proxy_process() {
        let root = Path::new("/workspace/exo2");
        let target_exe = root.join("target/debug/exo");
        let target_proxy_exe = root.join("target/debug/exo-mcp");
        let command = target_proxy_exe.display().to_string();

        let decision = mcp_process_decision(
            42,
            &command,
            Some(root),
            42,
            None,
            &target_exe,
            &target_proxy_exe,
            root,
        );

        assert_eq!(decision, McpProcessDecision::SkipSelf);
    }

    #[test]
    fn restart_mcp_scan_skips_current_parent_exo_mcp_proxy_process() {
        let root = Path::new("/workspace/exo2");
        let target_exe = root.join("target/debug/exo");
        let target_proxy_exe = root.join("target/debug/exo-mcp");
        let command = target_proxy_exe.display().to_string();

        let decision = mcp_process_decision(
            41,
            &command,
            Some(root),
            42,
            Some(41),
            &target_exe,
            &target_proxy_exe,
            root,
        );

        assert_eq!(decision, McpProcessDecision::SkipSelf);
    }

    #[test]
    fn restart_mcp_scan_ignores_non_mcp_command_with_exo_mcp_argument() {
        let root = Path::new("/workspace/exo2");
        let target_exe = root.join("target/debug/exo");
        let target_proxy_exe = root.join("target/debug/exo-mcp");

        let decision = mcp_process_decision(
            41,
            "vim exo-mcp",
            Some(root),
            42,
            None,
            &target_exe,
            &target_proxy_exe,
            root,
        );

        assert_eq!(decision, McpProcessDecision::Ignore);
    }
}
