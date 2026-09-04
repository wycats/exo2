//! Process creation coordinated with locald publisher descriptor acquisition where available.

#[cfg(unix)]
use locald_publisher_client::ProcessSpawnBarrier;
use std::io::{self, Read};
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
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("child stdout was not piped"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("child stderr was not piped"))?;
        let stdout = std::thread::spawn(move || drain_output(stdout));
        let stderr = std::thread::spawn(move || drain_output(stderr));
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(Output {
                    status,
                    stdout: join_output(stdout, "stdout")?,
                    stderr: join_output(stderr, "stderr")?,
                });
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                let _ = join_output(stdout, "stdout");
                let _ = join_output(stderr, "stderr");
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

fn drain_output(mut output: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    output.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn join_output(
    reader: std::thread::JoinHandle<io::Result<Vec<u8>>>,
    stream: &str,
) -> io::Result<Vec<u8>> {
    reader
        .join()
        .map_err(|_| io::Error::other(format!("{stream} reader panicked")))?
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
    fn guarded_timeout_drains_large_output_while_the_child_runs() {
        let output = Command::new("sh")
            .args([
                "-c",
                "head -c 1048576 /dev/zero; head -c 1048576 /dev/zero >&2",
            ])
            .output_guarded_timeout(Duration::from_secs(5))
            .expect("large output should not fill a pipe and time out");
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), 1_048_576);
        assert_eq!(output.stderr.len(), 1_048_576);
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
