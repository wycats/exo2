fn main() {
    println!("cargo:rerun-if-changed=windows-as-invoker.manifest");

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }

    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let manifest_path = std::path::Path::new(&manifest_dir).join("windows-as-invoker.manifest");
    let manifest_path = msvc_manifest_input_path(&manifest_path);
    let manifest_arg = format!("/MANIFESTINPUT:{}", manifest_path.display());

    for target in ["bins", "tests"] {
        println!("cargo:rustc-link-arg-{target}=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-{target}={manifest_arg}");
    }
}

fn msvc_manifest_input_path(path: &std::path::Path) -> std::path::PathBuf {
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
fn path_has_spaces(path: &std::path::Path) -> bool {
    path.to_string_lossy().contains(' ')
}

#[cfg(windows)]
fn windows_short_path(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let command = format!("for %I in (\"{}\") do @echo %~sI", path.display());
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", &command])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.trim();
    (!path.is_empty()).then(|| std::path::PathBuf::from(path))
}

#[cfg(windows)]
fn copy_manifest_to_space_free_temp(path: &std::path::Path) -> Option<std::path::PathBuf> {
    let temp_dir = windows_short_path(&std::env::temp_dir()).unwrap_or_else(std::env::temp_dir);
    if path_has_spaces(&temp_dir) {
        return None;
    }

    let target = temp_dir.join(format!(
        "exo-windows-as-invoker-{}.manifest",
        std::process::id()
    ));
    std::fs::copy(path, &target).ok()?;
    Some(target)
}
