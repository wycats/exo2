//! Process creation coordinated with locald publisher descriptor acquisition where available.

#[cfg(unix)]
use locald_publisher_client::ProcessSpawnBarrier;
use std::io;
use std::process::{Child, Command, ExitStatus, Output, Stdio};

/// Guarded process-creation operations for standard-library commands.
pub trait CommandSpawnExt {
    /// Spawn while holding the shared process permit only across child creation.
    fn spawn_guarded(&mut self) -> io::Result<Child>;

    /// Run to completion with the same I/O behavior as [`Command::output`].
    fn output_guarded(&mut self) -> io::Result<Output>;

    /// Run to completion with the same I/O behavior as [`Command::status`].
    fn status_guarded(&mut self) -> io::Result<ExitStatus>;

    /// Replace the current Unix process while holding the shared process permit.
    #[cfg(unix)]
    fn exec_guarded(&mut self) -> io::Error;
}

impl CommandSpawnExt for Command {
    fn spawn_guarded(&mut self) -> io::Result<Child> {
        #[cfg(unix)]
        {
            ProcessSpawnBarrier::global().spawn_std_command(self)
        }
        #[cfg(not(unix))]
        {
            self.spawn()
        }
    }

    fn output_guarded(&mut self) -> io::Result<Output> {
        self.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        self.spawn_guarded()?.wait_with_output()
    }

    fn status_guarded(&mut self) -> io::Result<ExitStatus> {
        self.spawn_guarded()?.wait()
    }

    #[cfg(unix)]
    fn exec_guarded(&mut self) -> io::Error {
        use std::os::unix::process::CommandExt as _;

        let _permit = ProcessSpawnBarrier::global().enter_spawn();
        self.exec()
    }
}

/// Guarded process creation for Tokio commands.
pub trait TokioCommandSpawnExt {
    /// Spawn while holding the shared process permit only across child creation.
    fn spawn_guarded(&mut self) -> io::Result<tokio::process::Child>;
}

impl TokioCommandSpawnExt for tokio::process::Command {
    fn spawn_guarded(&mut self) -> io::Result<tokio::process::Child> {
        #[cfg(unix)]
        {
            ProcessSpawnBarrier::global().spawn_tokio_command(self)
        }
        #[cfg(not(unix))]
        {
            self.spawn()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_output_preserves_captured_output_semantics() {
        let output = Command::new("sh")
            .args(["-c", "printf stdout; printf stderr >&2"])
            .output_guarded()
            .expect("guarded output");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"stdout");
        assert_eq!(output.stderr, b"stderr");
    }
}
