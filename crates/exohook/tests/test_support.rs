#![allow(dead_code, unused_imports)]

use std::io::{Read, Write};
use std::path::Path;

pub mod fs {
    use super::*;

    pub fn write(path: impl AsRef<Path>, contents: &str) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        file.write_all(contents.as_bytes())
    }

    pub fn read_to_string(path: impl AsRef<Path>) -> std::io::Result<String> {
        let mut file = std::fs::File::open(path)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        Ok(buf)
    }

    pub use std::fs::create_dir_all;
}

pub fn git(root: &Path, args: &[&str]) {
    let mut cmd = assert_cmd::Command::new("git");
    cmd.current_dir(root).args(args).assert().success();
}

pub fn git_output(root: &Path, args: &[&str]) -> String {
    let mut cmd = assert_cmd::Command::new("git");
    let assert = cmd.current_dir(root).args(args).assert().success();
    String::from_utf8_lossy(&assert.get_output().stdout).to_string()
}
