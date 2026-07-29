use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

fn main() {
    configure_windows_manifest();

    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_UI");
    println!("cargo:rerun-if-changed=../../packages/exosuit-cockpit/src");
    println!("cargo:rerun-if-changed=../../packages/exosuit-cockpit/package.json");
    println!("cargo:rerun-if-changed=../../packages/exosuit-cockpit/svelte.config.js");
    println!("cargo:rerun-if-changed=../../packages/exosuit-cockpit/vite.config.ts");
    println!("cargo:rerun-if-changed=../../package.json");
    println!("cargo:rerun-if-changed=../../pnpm-lock.yaml");
    println!("cargo:rerun-if-changed=../../pnpm-workspace.yaml");

    if env::var_os("CARGO_FEATURE_UI").is_none() {
        return;
    }

    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.join("../..");
    let cockpit_root = repo_root.join("packages/exosuit-cockpit");
    let cockpit_source = cockpit_root.join("src");
    let cockpit_build = cockpit_root.join("build");
    let out_assets =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR")).join("workbench-assets");

    if !cockpit_build.exists()
        || newest_mtime(&cockpit_source)
            .into_iter()
            .chain(newest_mtime(&cockpit_root.join("package.json")))
            .chain(newest_mtime(&cockpit_root.join("svelte.config.js")))
            .chain(newest_mtime(&cockpit_root.join("vite.config.ts")))
            .chain(newest_mtime(&repo_root.join("package.json")))
            .chain(newest_mtime(&repo_root.join("pnpm-lock.yaml")))
            .chain(newest_mtime(&repo_root.join("pnpm-workspace.yaml")))
            .max()
            .zip(newest_mtime(&cockpit_build))
            .is_none_or(|(input, output)| input > output)
    {
        run_pnpm(&repo_root, &["install", "--frozen-lockfile"]);
        run_pnpm(&repo_root, &["--filter", "exosuit-cockpit", "build"]);
    }

    assert!(
        cockpit_build.join("index.html").exists(),
        "cockpit build did not produce {}",
        cockpit_build.join("index.html").display()
    );

    if out_assets.exists() {
        fs::remove_dir_all(&out_assets).expect("remove stale embedded workbench assets");
    }
    copy_dir(&cockpit_build, &out_assets).expect("copy embedded workbench assets");
}

fn configure_windows_manifest() {
    println!("cargo:rerun-if-changed=windows-as-invoker.manifest");

    if env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }

    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let manifest_path = Path::new(&manifest_dir).join("windows-as-invoker.manifest");
    let manifest_path = msvc_manifest_input_path(&manifest_path);
    let manifest_arg = format!("/MANIFESTINPUT:{}", manifest_path.display());

    for target in ["bins", "tests"] {
        println!("cargo:rustc-link-arg-{target}=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-{target}={manifest_arg}");
    }
}

fn msvc_manifest_input_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        if path.to_string_lossy().contains(' ') {
            if let Some(path) = windows_short_path(path).filter(|path| !path_has_spaces(path)) {
                return path;
            }
            if let Some(path) = copy_manifest_to_space_free_temp(path) {
                return path;
            }
        }
    }

    path.to_path_buf()
}

#[cfg(windows)]
fn path_has_spaces(path: &Path) -> bool {
    path.to_string_lossy().contains(' ')
}

#[cfg(windows)]
fn windows_short_path(path: &Path) -> Option<PathBuf> {
    let command = format!("for %I in (\"{}\") do @echo %~sI", path.display());
    let output = Command::new("cmd.exe")
        .args(["/d", "/c", &command])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

#[cfg(windows)]
fn copy_manifest_to_space_free_temp(path: &Path) -> Option<PathBuf> {
    let temp_dir = windows_short_path(&env::temp_dir()).unwrap_or_else(env::temp_dir);
    if path_has_spaces(&temp_dir) {
        return None;
    }

    let target = temp_dir.join(format!(
        "exo-windows-as-invoker-{}.manifest",
        std::process::id()
    ));
    fs::copy(path, &target).ok()?;
    Some(target)
}

#[allow(clippy::disallowed_methods)] // build scripts cannot use Tokio process APIs
fn run_pnpm(repo_root: &Path, args: &[&str]) {
    let status = Command::new("pnpm")
        .args(args)
        .current_dir(repo_root)
        .status()
        .unwrap_or_else(|error| panic!("failed to run pnpm {}: {error}", args.join(" ")));
    assert!(
        status.success(),
        "pnpm {} failed with {status}",
        args.join(" ")
    );
}

fn newest_mtime(path: &Path) -> Option<SystemTime> {
    if path.is_file() {
        return fs::metadata(path).ok()?.modified().ok();
    }
    let mut newest = None;
    for entry in fs::read_dir(path).ok()?.flatten() {
        if let Some(modified) = newest_mtime(&entry.path())
            && newest.is_none_or(|current| modified > current)
        {
            newest = Some(modified);
        }
    }
    newest
}

fn copy_dir(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
