#![allow(missing_docs)]
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result, bail};
use serde::Serialize;

const USAGE: &str = "\
cargo dogfood-exo

Runs the full Exosuit dogfood refresh lifecycle:
- install JS dependencies
- build target/debug/exo and target/debug/exo-mcp
- install exo and exo-mcp into the Cargo install root
- rebuild generated/package artifacts
- install the dogfood extension
- restart scoped Exo runtimes
- verify and refresh the dogfood receipt
";
const EXO_PACKAGE_PATH: &str = "tools/exo";
const DOGFOOD_ACTIVATION_ENV: &str = "EXO_DOGFOOD_ACTIVATION";
const DOGFOOD_ACTIVATION_VERSION: u32 = 1;

fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print!("{USAGE}");
        return Ok(());
    }
    if let Some(arg) = args.first() {
        bail!("unexpected argument `{arg}`\n\n{USAGE}");
    }

    let root = exo_workspace::workspace_root()?;
    let exo_bin = exo_debug_binary(&root);
    let install_root = cargo_install_root(&root)?;

    step("Installing JS dependencies");
    exo_workspace::run_command("pnpm", &["install", "--frozen-lockfile"], &root)?;

    #[cfg(windows)]
    {
        step("Stopping debug Exo runtimes before rebuild");
        stop_windows_debug_exo(&root, &exo_bin)?;
    }

    step("Building exo binaries");
    exo_workspace::run_command("cargo", &["build", "-p", "exo", "--bins"], &root)?;

    #[cfg(windows)]
    {
        step("Restarting scoped Exo runtimes before install");
        exo_workspace::run_command(&exo_bin, &["--direct", "dogfood", "restart"], &root)?;

        step("Stopping installed Exo runtimes before install");
        stop_windows_installed_exo(&root, &install_root)?;
    }

    step("Installing exo binaries into Cargo install root");
    install_exo_binaries(&root, &install_root)?;
    let installed_exo_bin = installed_exo_path(&install_root)?;
    let activation = write_dogfood_activation(&root, &install_root)?;

    step("Pinning local Codex Exo plugin MCP command");
    pin_local_codex_exo_plugin_mcp(&root, &install_root, &activation)?;

    step("Building WASM bindings");
    run_workspace_helper(&root, "build_wasm")?;

    step("Generating command spec artifact");
    exo_workspace::run_command(&exo_bin, &["--direct", "json", "artifact"], &root)?;

    step("Curating language model tools");
    exo_workspace::run_command("node", &["scripts/sync-lm-tools.ts", "--add"], &root)?;

    step("Building workspace TypeScript dependencies");
    exo_workspace::run_command(
        "pnpm",
        &[
            "-r",
            "--filter",
            "@exosuit/core",
            "--filter",
            "@exosuit/rtd",
            "run",
            "build",
        ],
        &root,
    )?;

    step("Building extension bundle");
    exo_workspace::run_command(
        "pnpm",
        &["-C", "packages/exosuit-vscode", "run", "build:dogfood"],
        &root,
    )?;

    step("Packaging and installing extension");
    exo_workspace::run_command("node", &["scripts/dev/install-extension.ts"], &root)?;

    step("Restarting scoped Exo runtimes");
    run_installed_exo_command(
        &root,
        &install_root,
        &installed_exo_bin,
        &["--direct", "dogfood", "restart"],
    )?;

    step("Verifying activation before receipt");
    run_installed_exo_command_with_activation(
        &root,
        &install_root,
        &installed_exo_bin,
        &activation,
        &["--direct", "dogfood", "verify", "--skip-receipt"],
    )?;

    step("Writing dogfood receipt");
    run_installed_exo_command_with_activation(
        &root,
        &install_root,
        &installed_exo_bin,
        &activation,
        &["--direct", "dogfood", "receipt"],
    )?;

    step("Verifying receipt");
    run_installed_exo_command_with_activation(
        &root,
        &install_root,
        &installed_exo_bin,
        &activation,
        &["--direct", "dogfood", "verify"],
    )?;

    println!("Dogfood lifecycle complete.");
    Ok(())
}

fn step(label: &str) {
    println!("=== {label} ===");
}

fn run_workspace_helper(root: &Path, bin: &str) -> Result<()> {
    let args: Vec<OsString> = exo_workspace::cargo_bin_args(bin);
    exo_workspace::run_command("cargo", &args, root)
}

fn exo_debug_binary(root: &Path) -> PathBuf {
    debug_binary(root, "exo")
}

fn debug_binary(root: &Path, stem: &str) -> PathBuf {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    root.join("target/debug").join(format!("{stem}{suffix}"))
}

fn install_exo_binaries(root: &Path, install_root: &Path) -> Result<()> {
    let args: Vec<OsString> = vec![
        "install".into(),
        "--path".into(),
        EXO_PACKAGE_PATH.into(),
        "--locked".into(),
        "--root".into(),
        install_root.as_os_str().to_os_string(),
    ];
    exo_workspace::run_command("cargo", &args, root)
}

fn run_installed_exo_command(
    root: &Path,
    install_root: &Path,
    exo_bin: &Path,
    args: &[&str],
) -> Result<()> {
    let envs = installed_exo_command_env(install_root)?;
    exo_workspace::run_command_env_os(exo_bin, args, root, &envs)
}

fn run_installed_exo_command_with_activation(
    root: &Path,
    install_root: &Path,
    exo_bin: &Path,
    activation: &Path,
    args: &[&str],
) -> Result<()> {
    let mut envs = installed_exo_command_env(install_root)?;
    envs.push((
        DOGFOOD_ACTIVATION_ENV.into(),
        activation.as_os_str().to_os_string(),
    ));
    exo_workspace::run_command_env_os(exo_bin, args, root, &envs)
}

fn installed_exo_command_env(install_root: &Path) -> Result<Vec<(OsString, OsString)>> {
    Ok(vec![(
        "PATH".into(),
        path_env_with_install_bin(install_root)?,
    )])
}

fn path_env_with_install_bin(install_root: &Path) -> Result<OsString> {
    path_env_with_install_bin_from(install_root, std::env::var_os("PATH"))
}

fn path_env_with_install_bin_from(
    install_root: &Path,
    existing_path: Option<OsString>,
) -> Result<OsString> {
    let mut paths = vec![install_root.join("bin")];
    if let Some(existing_path) = existing_path {
        paths.extend(std::env::split_paths(&existing_path));
    }
    std::env::join_paths(paths).context("failed to prepend Cargo install bin directory to PATH")
}

fn pin_local_codex_exo_plugin_mcp(
    root: &Path,
    install_root: &Path,
    activation: &Path,
) -> Result<()> {
    let proxy = installed_exo_mcp_path(install_root)?;
    let paths = installed_codex_exo_plugin_mcp_paths(root)?;
    if paths.is_empty() {
        println!(
            "No installed Codex Exo plugin cache found; source plugin MCP config remains portable."
        );
        return Ok(());
    }

    for path in paths {
        write_pinned_mcp_config(&path, &proxy, activation)?;
        println!(
            "Pinned Codex Exo plugin MCP command at {} -> {} with dogfood build activation",
            path.display(),
            proxy.display()
        );
    }
    Ok(())
}

fn installed_exo_path(install_root: &Path) -> Result<PathBuf> {
    installed_binary_path(install_root, "exo")
}

fn installed_exo_mcp_path(install_root: &Path) -> Result<PathBuf> {
    installed_binary_path(install_root, "exo-mcp")
}

fn installed_binary_path(install_root: &Path, stem: &str) -> Result<PathBuf> {
    let path = install_root
        .join("bin")
        .join(format!("{stem}{}", std::env::consts::EXE_SUFFIX));
    if !path.is_file() {
        bail!(
            "installed {stem} binary was not found at {} after cargo install",
            path.display()
        );
    }
    Ok(path.canonicalize().unwrap_or(path))
}

fn cargo_install_root(root: &Path) -> Result<PathBuf> {
    cargo_install_root_from(
        root,
        cargo_config2::cargo_home_with_cwd(root).as_deref(),
        std::env::var_os("CARGO_INSTALL_ROOT").map(PathBuf::from),
    )
}

fn cargo_install_root_from(
    root: &Path,
    cargo_home: Option<&Path>,
    env_install_root: Option<PathBuf>,
) -> Result<PathBuf> {
    if let Some(env_root) = env_install_root {
        return Ok(resolve_install_root_from_workspace(root, &env_root));
    }
    if let Some(root) = cargo_config_install_root(root, cargo_home)? {
        return Ok(root);
    }
    cargo_home
        .map(|cargo_home| resolve_install_root_from_workspace(root, cargo_home))
        .context("failed to resolve Cargo install root from CARGO_INSTALL_ROOT, install.root, CARGO_HOME, USERPROFILE, or HOME")
}

fn resolve_install_root_from_workspace(workspace_root: &Path, install_root: &Path) -> PathBuf {
    if install_root.is_absolute() {
        install_root.to_path_buf()
    } else {
        workspace_root.join(install_root)
    }
}

fn cargo_config_install_root(root: &Path, cargo_home: Option<&Path>) -> Result<Option<PathBuf>> {
    let mut selected = None;
    let package_root = root.join(EXO_PACKAGE_PATH);
    for config in cargo_config_paths(&package_root, cargo_home) {
        if let Some(root) = install_root_from_cargo_config(&config)? {
            selected = Some(root);
        }
    }
    Ok(selected)
}

fn cargo_config_paths(root: &Path, cargo_home: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = cargo_config2::Walk::with_cargo_home(root, cargo_home.map(Path::to_path_buf))
        .collect::<Vec<_>>();
    paths.reverse();
    paths
}

fn install_root_from_cargo_config(path: &Path) -> Result<Option<PathBuf>> {
    install_root_from_cargo_config_inner(path, &mut HashSet::new())
}

fn install_root_from_cargo_config_inner(
    path: &Path,
    loading: &mut HashSet<PathBuf>,
) -> Result<Option<PathBuf>> {
    if !path.is_file() {
        return Ok(None);
    }
    let identity = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !loading.insert(identity.clone()) {
        bail!("Cargo config include cycle detected at {}", path.display());
    }

    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let value = toml::from_str::<toml::Table>(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;

    let mut selected = None;
    for include in cargo_config_includes(path, value.get("include"))? {
        if !include.path.is_file() {
            if include.optional {
                continue;
            }
            bail!(
                "required Cargo config include was not found: {}",
                include.path.display()
            );
        }
        if let Some(root) = install_root_from_cargo_config_inner(&include.path, loading)? {
            selected = Some(root);
        }
    }

    if let Some(root) = install_root_from_cargo_config_value(path, &value)? {
        selected = Some(root);
    }

    loading.remove(&identity);
    Ok(selected)
}

struct CargoConfigInclude {
    path: PathBuf,
    optional: bool,
}

fn cargo_config_includes(
    config_path: &Path,
    value: Option<&toml::Value>,
) -> Result<Vec<CargoConfigInclude>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let includes = value.as_array().with_context(|| {
        format!(
            "Cargo config `include` in {} must be an array",
            config_path.display()
        )
    })?;

    includes
        .iter()
        .map(|include| cargo_config_include(config_path, include))
        .collect()
}

fn cargo_config_include(config_path: &Path, include: &toml::Value) -> Result<CargoConfigInclude> {
    let (path, optional) = match include {
        toml::Value::String(path) => (path.as_str(), false),
        toml::Value::Table(table) => {
            let path = table
                .get("path")
                .and_then(toml::Value::as_str)
                .with_context(|| {
                    format!(
                        "Cargo config include table in {} is missing string field `path`",
                        config_path.display()
                    )
                })?;
            let optional = match table.get("optional") {
                Some(value) => value.as_bool().with_context(|| {
                    format!(
                        "Cargo config include table in {} has non-boolean field `optional`",
                        config_path.display()
                    )
                })?,
                None => false,
            };
            (path, optional)
        }
        _ => bail!(
            "Cargo config include in {} must be a string or table",
            config_path.display()
        ),
    };

    if !path.ends_with(".toml") {
        bail!(
            "Cargo config include in {} must end with .toml: {}",
            config_path.display(),
            path
        );
    }

    let path = PathBuf::from(path);
    let path = if path.is_absolute() {
        path
    } else {
        config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(path)
    };

    Ok(CargoConfigInclude { path, optional })
}

fn install_root_from_cargo_config_value(
    path: &Path,
    value: &toml::Table,
) -> Result<Option<PathBuf>> {
    let Some(root) = value
        .get("install")
        .and_then(|install| install.get("root"))
        .and_then(toml::Value::as_str)
    else {
        return Ok(None);
    };
    let root = PathBuf::from(root);
    if root.is_absolute() {
        Ok(Some(root))
    } else {
        Ok(Some(cargo_config_relative_base(path).join(root)))
    }
}

fn cargo_config_relative_base(path: &Path) -> &Path {
    path.parent()
        .and_then(Path::parent)
        .or_else(|| path.parent())
        .unwrap_or_else(|| Path::new("."))
}

fn installed_codex_exo_plugin_mcp_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let Some((name, version)) = source_plugin_identity(root)? else {
        return Ok(Vec::new());
    };
    let Some(codex_home) = codex_home() else {
        return Ok(Vec::new());
    };
    let cache_root = codex_home.join("plugins").join("cache");
    plugin_cache_mcp_paths(&cache_root, &name, &version)
}

fn source_plugin_identity(root: &Path) -> Result<Option<(String, String)>> {
    let path = root.join("plugins/exo/.codex-plugin/plugin.json");
    if !path.is_file() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let name = value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .context("plugin.json is missing string field `name`")?;
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .context("plugin.json is missing string field `version`")?;
    Ok(Some((name.to_string(), version.to_string())))
}

fn plugin_cache_mcp_paths(cache_root: &Path, name: &str, version: &str) -> Result<Vec<PathBuf>> {
    if !cache_root.is_dir() {
        return Ok(Vec::new());
    }

    let mut paths = Vec::new();
    for entry in fs::read_dir(cache_root)
        .with_context(|| format!("failed to read {}", cache_root.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", cache_root.display()))?;
        if !entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            continue;
        }
        let path = entry.path().join(name).join(version).join(".mcp.json");
        if path.is_file() {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn write_pinned_mcp_config(path: &Path, proxy: &Path, activation: &Path) -> Result<()> {
    let mut env = serde_json::Map::new();
    env.insert(
        DOGFOOD_ACTIVATION_ENV.to_string(),
        serde_json::Value::String(activation.display().to_string()),
    );
    let value = serde_json::json!({
        "mcpServers": {
            "exo": {
                "command": proxy.display().to_string(),
                "args": [],
                "env": env
            }
        }
    });
    let content = format!("{}\n", serde_json::to_string_pretty(&value)?);
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

#[derive(Debug, Serialize)]
struct DogfoodActivation {
    version: u32,
    source: DogfoodActivationBinaries,
    installed: DogfoodActivationBinaries,
}

#[derive(Debug, Serialize)]
struct DogfoodActivationBinaries {
    exo: DogfoodActivationBinary,
    exo_mcp: DogfoodActivationBinary,
}

#[derive(Debug, Serialize)]
struct DogfoodActivationBinary {
    path: PathBuf,
    blake3: String,
    size_bytes: u64,
    modified_unix_ms: Option<u128>,
}

fn write_dogfood_activation(root: &Path, install_root: &Path) -> Result<PathBuf> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let activation_dir = install_root.join("exo-dogfood");
    fs::create_dir_all(&activation_dir)
        .with_context(|| format!("failed to create {}", activation_dir.display()))?;
    let root_hash = blake3::hash(root.as_os_str().as_encoded_bytes()).to_hex();
    let path = activation_dir.join(format!("{}.json", &root_hash[..16]));
    let activation = DogfoodActivation {
        version: DOGFOOD_ACTIVATION_VERSION,
        source: DogfoodActivationBinaries {
            exo: dogfood_activation_binary(&exo_debug_binary(&root))?,
            exo_mcp: dogfood_activation_binary(&debug_binary(&root, "exo-mcp"))?,
        },
        installed: DogfoodActivationBinaries {
            exo: dogfood_activation_binary(&installed_exo_path(install_root)?)?,
            exo_mcp: dogfood_activation_binary(&installed_exo_mcp_path(install_root)?)?,
        },
    };
    let serialized = serde_json::to_vec_pretty(&activation)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serialized)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    fs::rename(&temporary, &path)
        .with_context(|| format!("failed to publish {}", path.display()))?;
    Ok(path)
}

fn dogfood_activation_binary(path: &Path) -> Result<DogfoodActivationBinary> {
    let path = path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", path.display()))?;
    let metadata =
        fs::metadata(&path).with_context(|| format!("failed to stat {}", path.display()))?;
    Ok(DogfoodActivationBinary {
        blake3: blake3::hash(
            &fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?,
        )
        .to_hex()
        .to_string(),
        size_bytes: metadata.len(),
        modified_unix_ms: metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis()),
        path,
    })
}

fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| user_home_dir().map(|home| home.join(".codex")))
}

fn user_home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
    }

    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

#[cfg(windows)]
fn stop_windows_debug_exo(root: &Path, exo_bin: &Path) -> Result<()> {
    for target in windows_debug_cleanup_targets(root, exo_bin) {
        stop_windows_processes_for_executable(root, &target)?;
    }
    Ok(())
}

#[cfg(windows)]
fn windows_debug_cleanup_targets(root: &Path, exo_bin: &Path) -> [PathBuf; 2] {
    [exo_bin.to_path_buf(), debug_binary(root, "exo-mcp")]
}

#[cfg(windows)]
fn stop_windows_installed_exo(root: &Path, install_root: &Path) -> Result<()> {
    for target in windows_installed_cleanup_targets(install_root) {
        stop_windows_processes_for_executable(root, &target)?;
    }
    Ok(())
}

#[cfg(windows)]
fn windows_installed_cleanup_targets(install_root: &Path) -> [PathBuf; 2] {
    [
        install_root.join("bin").join("exo.exe"),
        install_root.join("bin").join("exo-mcp.exe"),
    ]
}

#[cfg(windows)]
fn stop_windows_processes_for_executable(root: &Path, target: &Path) -> Result<()> {
    if !target.exists() {
        return Ok(());
    }

    let script = windows_stop_process_script(target);
    let args: Vec<OsString> = vec!["-NoProfile".into(), "-Command".into(), script.into()];
    exo_workspace::run_command("powershell.exe", &args, root)
}

#[cfg(windows)]
fn windows_stop_process_script(target: &Path) -> String {
    let target = powershell_single_quoted(&target.display().to_string());
    format!(
        r#"
$ErrorActionPreference = 'Stop'
function Normalize-ProcessPath([string]$Path) {{
  $full = [System.IO.Path]::GetFullPath($Path)
  if ($full.StartsWith('\\?\UNC\', [System.StringComparison]::OrdinalIgnoreCase)) {{
    return '\\' + $full.Substring(8)
  }}
  if ($full.StartsWith('\\?\', [System.StringComparison]::OrdinalIgnoreCase)) {{
    return $full.Substring(4)
  }}
  return $full
}}

$target = {target}
$targetFull = Normalize-ProcessPath $target
Get-CimInstance Win32_Process |
  Where-Object {{
    $_.ExecutablePath -and
    [System.String]::Equals(
      (Normalize-ProcessPath $_.ExecutablePath),
      $targetFull,
      [System.StringComparison]::OrdinalIgnoreCase
    )
  }} |
  ForEach-Object {{
    if ($_.ProcessId -ne $PID) {{
      Stop-Process -Id $_.ProcessId -Force
    }}
  }}
"#
    )
}

#[cfg(windows)]
fn powershell_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("exo-workspace-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    #[test]
    fn plugin_cache_mcp_paths_find_matching_installed_plugin_caches() {
        let root = temp_dir("plugin-cache-paths");
        let cache = root.join("cache");
        let first = cache.join("exo2").join("exo").join("0.1.0");
        let second = cache.join("local").join("exo").join("0.1.0");
        let ignored = cache.join("exo2").join("other").join("0.1.0");
        fs::create_dir_all(&first).expect("create first cache");
        fs::create_dir_all(&second).expect("create second cache");
        fs::create_dir_all(&ignored).expect("create ignored cache");
        fs::write(first.join(".mcp.json"), "{}").expect("write first mcp");
        fs::write(second.join(".mcp.json"), "{}").expect("write second mcp");
        fs::write(ignored.join(".mcp.json"), "{}").expect("write ignored mcp");

        let paths = plugin_cache_mcp_paths(&cache, "exo", "0.1.0").expect("scan plugin cache");

        assert_eq!(
            paths,
            vec![first.join(".mcp.json"), second.join(".mcp.json")]
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn write_pinned_mcp_config_uses_absolute_proxy_command_and_activation() {
        let root = temp_dir("pinned-mcp");
        let mcp = root.join(".mcp.json");
        let proxy = root.join(format!("exo-mcp{}", std::env::consts::EXE_SUFFIX));
        let activation = root.join("activation.json");

        write_pinned_mcp_config(&mcp, &proxy, &activation).expect("write pinned config");
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&mcp).expect("read mcp")).expect("valid json");

        assert_eq!(
            value["mcpServers"]["exo"]["command"],
            proxy.display().to_string()
        );
        assert_eq!(value["mcpServers"]["exo"]["args"], serde_json::json!([]));
        assert_eq!(
            value["mcpServers"]["exo"]["env"][DOGFOOD_ACTIVATION_ENV],
            activation.display().to_string()
        );
        assert_eq!(
            value["mcpServers"]["exo"]["env"]
                .as_object()
                .expect("MCP environment object")
                .len(),
            1
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn cargo_install_root_prefers_env_install_root() {
        let root = temp_dir("install-root-env");
        let cargo_home = root.join("cargo-home");
        let env_root = root.join("env-install-root");

        let install_root =
            cargo_install_root_from(&root, Some(&cargo_home), Some(env_root.clone()))
                .expect("resolve install root");

        assert_eq!(install_root, env_root);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn cargo_install_root_resolves_relative_env_root_from_workspace_root() {
        let root = temp_dir("install-root-relative-env");
        let cargo_home = root.join("cargo-home");
        let env_root = PathBuf::from("env-install-root");

        let install_root = cargo_install_root_from(&root, Some(&cargo_home), Some(env_root))
            .expect("resolve install root");

        assert_eq!(install_root, root.join("env-install-root"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn dogfood_path_env_prepends_install_bin() {
        let root = temp_dir("install-root-path-env");
        let install_root = root.join("install-root");
        let first_existing = root.join("first");
        let second_existing = root.join("second");
        let existing_path = std::env::join_paths([first_existing.clone(), second_existing.clone()])
            .expect("join existing PATH");

        let path = path_env_with_install_bin_from(&install_root, Some(existing_path))
            .expect("build dogfood PATH");
        let paths = std::env::split_paths(&path).collect::<Vec<_>>();

        assert_eq!(
            paths,
            vec![install_root.join("bin"), first_existing, second_existing]
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn cargo_install_root_uses_workspace_config_before_cargo_home_config() {
        let root = temp_dir("install-root-config");
        let cargo_home = root.join("cargo-home");
        let workspace_cargo = root.join(".cargo");
        let cargo_home_root = root.join("cargo-home-install-root");
        let workspace_root = root.join("workspace-install-root");
        fs::create_dir_all(&cargo_home).expect("create cargo home");
        fs::create_dir_all(&workspace_cargo).expect("create workspace cargo config dir");
        fs::write(
            cargo_home.join("config.toml"),
            format!("[install]\nroot = '{}'\n", cargo_home_root.display()),
        )
        .expect("write cargo home config");
        fs::write(
            workspace_cargo.join("config.toml"),
            format!("[install]\nroot = '{}'\n", workspace_root.display()),
        )
        .expect("write workspace config");

        let install_root =
            cargo_install_root_from(&root, Some(&cargo_home), None).expect("resolve install root");

        assert_eq!(install_root, workspace_root);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn cargo_install_root_resolves_relative_workspace_config_from_workspace_root() {
        let root = temp_dir("install-root-relative-config");
        let cargo_home = root.join("cargo-home");
        let workspace_cargo = root.join(".cargo");
        fs::create_dir_all(&workspace_cargo).expect("create workspace cargo config dir");
        fs::write(
            workspace_cargo.join("config.toml"),
            "[install]\nroot = 'install-root'\n",
        )
        .expect("write workspace config");

        let install_root =
            cargo_install_root_from(&root, Some(&cargo_home), None).expect("resolve install root");

        assert_eq!(install_root, root.join("install-root"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn cargo_install_root_prefers_extensionless_config_over_toml_sibling() {
        let root = temp_dir("install-root-extensionless-config");
        let cargo_home = root.join("cargo-home");
        let workspace_cargo = root.join(".cargo");
        fs::create_dir_all(&workspace_cargo).expect("create workspace cargo config dir");
        fs::write(
            workspace_cargo.join("config"),
            "[install]\nroot = 'extensionless-install-root'\n",
        )
        .expect("write extensionless workspace config");
        fs::write(
            workspace_cargo.join("config.toml"),
            "[install]\nroot = 'toml-install-root'\n",
        )
        .expect("write toml workspace config");

        let install_root =
            cargo_install_root_from(&root, Some(&cargo_home), None).expect("resolve install root");

        assert_eq!(install_root, root.join("extensionless-install-root"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn cargo_install_root_uses_parent_config_before_cargo_home_config() {
        let parent = temp_dir("install-root-parent-config");
        let root = parent.join("workspace");
        let cargo_home = parent.join("cargo-home");
        let parent_cargo = parent.join(".cargo");
        let cargo_home_root = parent.join("cargo-home-install-root");
        fs::create_dir_all(&root).expect("create workspace root");
        fs::create_dir_all(&cargo_home).expect("create cargo home");
        fs::create_dir_all(&parent_cargo).expect("create parent cargo config dir");
        fs::write(
            cargo_home.join("config.toml"),
            format!("[install]\nroot = '{}'\n", cargo_home_root.display()),
        )
        .expect("write cargo home config");
        fs::write(
            parent_cargo.join("config.toml"),
            "[install]\nroot = 'parent-install-root'\n",
        )
        .expect("write parent config");

        let install_root =
            cargo_install_root_from(&root, Some(&cargo_home), None).expect("resolve install root");

        assert_eq!(install_root, parent.join("parent-install-root"));
        fs::remove_dir_all(parent).ok();
    }

    #[test]
    fn cargo_install_root_uses_package_config_before_workspace_config() {
        let root = temp_dir("install-root-package-config");
        let cargo_home = root.join("cargo-home");
        let workspace_cargo = root.join(".cargo");
        let package_cargo = root.join(EXO_PACKAGE_PATH).join(".cargo");
        fs::create_dir_all(&workspace_cargo).expect("create workspace cargo config dir");
        fs::create_dir_all(&package_cargo).expect("create package cargo config dir");
        fs::write(
            workspace_cargo.join("config.toml"),
            "[install]\nroot = 'workspace-install-root'\n",
        )
        .expect("write workspace config");
        fs::write(
            package_cargo.join("config.toml"),
            "[install]\nroot = 'package-install-root'\n",
        )
        .expect("write package config");

        let install_root =
            cargo_install_root_from(&root, Some(&cargo_home), None).expect("resolve install root");

        assert_eq!(
            install_root,
            root.join(EXO_PACKAGE_PATH).join("package-install-root")
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn cargo_install_root_uses_included_config_install_root() {
        let root = temp_dir("install-root-included-config");
        let cargo_home = root.join("cargo-home");
        let workspace_cargo = root.join(".cargo");
        fs::create_dir_all(&workspace_cargo).expect("create workspace cargo config dir");
        fs::write(
            workspace_cargo.join("config.toml"),
            "include = ['install-root.toml']\n",
        )
        .expect("write workspace config");
        fs::write(
            workspace_cargo.join("install-root.toml"),
            "[install]\nroot = 'included-install-root'\n",
        )
        .expect("write included config");

        let install_root =
            cargo_install_root_from(&root, Some(&cargo_home), None).expect("resolve install root");

        assert_eq!(install_root, root.join("included-install-root"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn cargo_install_root_prefers_config_value_over_included_config() {
        let root = temp_dir("install-root-include-overridden");
        let cargo_home = root.join("cargo-home");
        let workspace_cargo = root.join(".cargo");
        fs::create_dir_all(&workspace_cargo).expect("create workspace cargo config dir");
        fs::write(
            workspace_cargo.join("config.toml"),
            "include = ['install-root.toml']\n[install]\nroot = 'local-install-root'\n",
        )
        .expect("write workspace config");
        fs::write(
            workspace_cargo.join("install-root.toml"),
            "[install]\nroot = 'included-install-root'\n",
        )
        .expect("write included config");

        let install_root =
            cargo_install_root_from(&root, Some(&cargo_home), None).expect("resolve install root");

        assert_eq!(install_root, root.join("local-install-root"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn cargo_install_root_falls_back_to_cargo_home() {
        let root = temp_dir("install-root-fallback");
        let cargo_home = root.join("cargo-home");

        let install_root =
            cargo_install_root_from(&root, Some(&cargo_home), None).expect("resolve install root");

        assert_eq!(install_root, cargo_home);
        fs::remove_dir_all(root).ok();
    }

    #[cfg(windows)]
    #[test]
    fn debug_cleanup_targets_include_exo_and_mcp() {
        let root = Path::new(r"C:\work\exo2");
        let exo_bin = debug_binary(root, "exo");

        assert_eq!(
            windows_debug_cleanup_targets(root, &exo_bin),
            [
                root.join(r"target\debug\exo.exe"),
                root.join(r"target\debug\exo-mcp.exe")
            ]
        );
    }

    #[cfg(windows)]
    #[test]
    fn installed_cleanup_targets_include_exo_and_mcp() {
        let install_root = Path::new(r"C:\cargo-install-root");

        assert_eq!(
            windows_installed_cleanup_targets(install_root),
            [
                install_root.join(r"bin\exo.exe"),
                install_root.join(r"bin\exo-mcp.exe")
            ]
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_stop_process_script_normalizes_extended_path_prefixes() {
        let script = windows_stop_process_script(Path::new(r"C:\cargo-install-root\bin\exo.exe"));

        assert!(script.contains("function Normalize-ProcessPath"));
        assert!(script.contains("StartsWith('\\\\?\\'"));
        assert!(script.contains("Normalize-ProcessPath $_.ExecutablePath"));
        assert!(script.contains(r"C:\cargo-install-root\bin\exo.exe"));
    }
}
