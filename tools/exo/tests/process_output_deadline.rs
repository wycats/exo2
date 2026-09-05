use exo::process_spawn::CommandSpawnExt;
use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn fixture_command(mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "output_fixture", "--nocapture"])
        .env("EXO_TEST_OUTPUT_MODE", mode);
    command
}

#[test]
fn output_fixture() {
    let Ok(mode) = std::env::var("EXO_TEST_OUTPUT_MODE") else {
        return;
    };
    if mode == "large_output" {
        let bytes = vec![0; 1_048_576];
        io::stdout().write_all(&bytes).unwrap();
        io::stderr().write_all(&bytes).unwrap();
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("EXO_TEST_OUTPUT_ROOT").unwrap());
    if mode == "holder" {
        std::fs::write(root.join("ready"), b"ready").unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !root.join("release").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        std::fs::write(root.join("done"), b"done").unwrap();
        return;
    }
    let mut holder = fixture_command("holder")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn_guarded()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !root.join("ready").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if mode == "parent_waits" {
        holder.wait().unwrap();
    }
}

#[test]
fn deadline_covers_inherited_pipes_after_parent_exit_or_timeout() {
    for mode in ["parent_exits", "parent_waits"] {
        let temp = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let result = fixture_command(mode)
            .env("EXO_TEST_OUTPUT_ROOT", temp.path())
            .output_guarded_timeout(Duration::from_secs(5));
        let elapsed = started.elapsed();
        // Always release the owned descendant fixture, including assertion failures.
        std::fs::write(temp.path().join("release"), b"release").unwrap();
        let cleanup_deadline = Instant::now() + Duration::from_secs(5);
        while !temp.path().join("done").exists() && Instant::now() < cleanup_deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            temp.path().join("ready").exists(),
            "descendant did not start: {result:?}"
        );
        assert!(
            temp.path().join("done").exists(),
            "descendant did not finish"
        );
        assert_eq!(
            result.unwrap_err().kind(),
            io::ErrorKind::TimedOut,
            "{mode}"
        );
        assert!(elapsed < Duration::from_secs(10), "{mode}: {elapsed:?}");
    }
}

#[test]
fn deadline_capture_drains_large_stdout_and_stderr() {
    let output = fixture_command("large_output")
        .output_guarded_timeout(Duration::from_secs(10))
        .unwrap();
    assert!(output.status.success());
    for bytes in [output.stdout, output.stderr] {
        assert_eq!(bytes.iter().filter(|&&byte| byte == 0).count(), 1_048_576);
    }
}
