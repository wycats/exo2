//! Process creation coordinated with locald publisher descriptor acquisition where available.

#[cfg(unix)]
use locald_publisher_client::ProcessSpawnBarrier;
use std::io;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

/// Guarded process-creation operations for standard-library commands.
pub trait CommandSpawnExt {
    /// Spawn while holding the shared process permit only across child creation.
    fn spawn_guarded(&mut self) -> io::Result<Child>;

    /// Run to completion with the same I/O behavior as [`Command::output`].
    fn output_guarded(&mut self) -> io::Result<Output>;

    /// Run to completion, killing and reaping the child when the deadline expires.
    fn output_guarded_timeout(&mut self, timeout: Duration) -> io::Result<Output>;

    /// Run to completion while preserving the caller's configured stdout and stderr.
    fn output_with_configured_stdio_guarded(&mut self) -> io::Result<Output>;

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

    fn output_guarded_timeout(&mut self, timeout: Duration) -> io::Result<Output> {
        self.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = self.spawn_guarded()?;
        let deadline = Instant::now() + timeout;
        loop {
            if child.try_wait()?.is_some() {
                return child.wait_with_output();
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("process exceeded {} ms timeout", timeout.as_millis()),
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn output_with_configured_stdio_guarded(&mut self) -> io::Result<Output> {
        self.stdin(Stdio::null());
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

    #[cfg(unix)]
    #[test]
    fn guarded_timeout_kills_and_reaps_the_child() {
        let started = Instant::now();
        let error = Command::new("sh")
            .args(["-c", "exec sleep 10"])
            .output_guarded_timeout(Duration::from_millis(50))
            .expect_err("child should time out");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn guarded_output_preserves_explicit_stderr_configuration() {
        let output = Command::new("sh")
            .args(["-c", "printf stdout; printf stderr >&2"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output_with_configured_stdio_guarded()
            .expect("guarded output with configured stdio");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"stdout");
        assert!(output.stderr.is_empty());
    }
}
