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
    /// The deadline also covers pipe EOF when descendants inherit captured output.
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
        #[cfg(not(any(unix, windows)))]
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "bounded process output is unavailable on this platform",
        ));

        #[cfg(any(unix, windows))]
        {
            self.stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = self.spawn_guarded()?;
            let result = (|| {
                let mut stdout = child
                    .stdout
                    .take()
                    .ok_or_else(|| io::Error::other("child stdout was not piped"))?;
                let mut stderr = child
                    .stderr
                    .take()
                    .ok_or_else(|| io::Error::other("child stderr was not piped"))?;
                stdout.prepare()?;
                stderr.prepare()?;
                let mut stdout_bytes = Vec::new();
                let mut stderr_bytes = Vec::new();
                let mut stdout_eof = false;
                let mut stderr_eof = false;
                let mut status = None;
                let deadline = Instant::now() + timeout;
                loop {
                    if Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            format!("process exceeded {} ms timeout", timeout.as_millis()),
                        ));
                    }
                    if status.is_none() {
                        status = child.try_wait()?;
                    }
                    let stdout_progress =
                        drain_available(&mut stdout, &mut stdout_bytes, &mut stdout_eof)?;
                    let stderr_progress =
                        drain_available(&mut stderr, &mut stderr_bytes, &mut stderr_eof)?;
                    if stdout_eof
                        && stderr_eof
                        && let Some(status) = status
                    {
                        return Ok(Output {
                            status,
                            stdout: stdout_bytes,
                            stderr: stderr_bytes,
                        });
                    }
                    if !stdout_progress && !stderr_progress {
                        std::thread::sleep(
                            Duration::from_millis(10)
                                .min(deadline.saturating_duration_since(Instant::now())),
                        );
                    }
                }
            })();
            if result.is_err() {
                let _ = child.kill();
                let _ = child.wait();
            }
            result
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

#[cfg(any(unix, windows))]
trait TimeoutPipe: Read {
    fn prepare(&mut self) -> io::Result<()>;
    fn read_available(&mut self, buffer: &mut [u8]) -> io::Result<Option<usize>>;
}

#[cfg(unix)]
impl<T: Read + std::os::fd::AsFd> TimeoutPipe for T {
    fn prepare(&mut self) -> io::Result<()> {
        use nix::fcntl::{FcntlArg, OFlag, fcntl};
        let flags = OFlag::from_bits_truncate(fcntl(&*self, FcntlArg::F_GETFL)?);
        fcntl(&*self, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
        Ok(())
    }

    fn read_available(&mut self, buffer: &mut [u8]) -> io::Result<Option<usize>> {
        match self.read(buffer) {
            Ok(count) => Ok(Some(count)),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(windows)]
impl<T: Read + std::os::windows::io::AsHandle> TimeoutPipe for T {
    fn prepare(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn read_available(&mut self, buffer: &mut [u8]) -> io::Result<Option<usize>> {
        use io_lifetimes::AsFilelike;
        // Peek through a borrowed handle, but consume through ChildStdout/ChildStderr:
        // std::fs::File::read does not support their overlapped Windows handles.
        let available =
            system_interface::io::IoExt::peek(&*self.as_filelike_view::<std::fs::File>(), buffer);
        match available {
            Ok(0) => Ok(None),
            Ok(count) => self.read(&mut buffer[..count]).map(Some),
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(Some(0)),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(None),
            Err(error) => Err(error),
        }
    }
}

#[cfg(any(unix, windows))]
fn drain_available(
    pipe: &mut impl TimeoutPipe,
    bytes: &mut Vec<u8>,
    eof: &mut bool,
) -> io::Result<bool> {
    if *eof {
        return Ok(false);
    }
    let mut buffer = [0; 8192];
    match pipe.read_available(&mut buffer)? {
        None => Ok(false),
        Some(0) => {
            *eof = true;
            Ok(false)
        }
        Some(count) => {
            bytes.extend_from_slice(&buffer[..count]);
            Ok(true)
        }
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
