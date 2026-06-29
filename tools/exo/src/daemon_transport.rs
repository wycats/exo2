use std::fmt;
use std::io::{self, Read, Write};
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};
#[cfg(windows)]
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[derive(Clone)]
pub struct DaemonEndpoint {
    #[cfg(unix)]
    path: PathBuf,
    #[cfg(windows)]
    pipe_name: String,
}

impl DaemonEndpoint {
    pub fn from_socket_path(socket_path: &Path) -> Self {
        #[cfg(unix)]
        {
            Self {
                path: socket_path.to_path_buf(),
            }
        }

        #[cfg(windows)]
        {
            let digest = blake3::hash(socket_path.to_string_lossy().as_bytes());
            Self {
                pipe_name: format!(r"\\.\pipe\exo-{}", &digest.to_hex()[..32]),
            }
        }
    }

    #[cfg(windows)]
    pub fn from_runtime_dir(runtime_dir: &Path) -> Self {
        let digest = blake3::hash(runtime_dir.to_string_lossy().as_bytes());
        Self {
            pipe_name: format!(r"\\.\pipe\exo-{}", &digest.to_hex()[..32]),
        }
    }

    pub fn display(&self) -> String {
        #[cfg(unix)]
        {
            self.path.display().to_string()
        }

        #[cfg(windows)]
        {
            self.pipe_name.clone()
        }
    }

    pub fn exists(&self) -> bool {
        #[cfg(unix)]
        {
            self.path.exists()
        }

        #[cfg(windows)]
        {
            false
        }
    }

    pub fn remove_stale(&self) -> io::Result<()> {
        #[cfg(unix)]
        {
            if self.path.exists() {
                std::fs::remove_file(&self.path)?;
            }
        }

        Ok(())
    }

    pub async fn connect(&self) -> io::Result<DaemonStream> {
        #[cfg(unix)]
        {
            tokio::net::UnixStream::connect(&self.path)
                .await
                .map(DaemonStream::unix)
        }

        #[cfg(windows)]
        {
            let start = Instant::now();
            loop {
                match tokio::net::windows::named_pipe::ClientOptions::new().open(&self.pipe_name) {
                    Ok(stream) => return Ok(DaemonStream::named_pipe_client(stream)),
                    Err(error)
                        if is_named_pipe_retryable(&error)
                            && start.elapsed() < Duration::from_secs(2) =>
                    {
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
    }

    pub fn connect_blocking(&self) -> io::Result<DaemonClientStream> {
        #[cfg(unix)]
        {
            std::os::unix::net::UnixStream::connect(&self.path).map(DaemonClientStream::unix)
        }

        #[cfg(windows)]
        {
            let start = Instant::now();
            loop {
                match std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&self.pipe_name)
                {
                    Ok(stream) => return Ok(DaemonClientStream::named_pipe(stream)),
                    Err(error)
                        if is_named_pipe_retryable(&error)
                            && start.elapsed() < Duration::from_secs(2) =>
                    {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    Err(error) => return Err(error),
                }
            }
        }
    }

    pub fn is_connectable_blocking(&self) -> bool {
        self.connect_blocking().is_ok()
    }

    pub async fn bind(&self) -> io::Result<DaemonListener> {
        #[cfg(unix)]
        {
            tokio::net::UnixListener::bind(&self.path).map(DaemonListener::unix)
        }

        #[cfg(windows)]
        {
            DaemonListener::named_pipe(&self.pipe_name)
        }
    }
}

impl fmt::Debug for DaemonEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DaemonEndpoint")
            .field("display", &self.display())
            .finish()
    }
}

pub struct DaemonListener {
    #[cfg(unix)]
    inner: tokio::net::UnixListener,
    #[cfg(windows)]
    pipe_name: String,
    #[cfg(windows)]
    next: Option<tokio::net::windows::named_pipe::NamedPipeServer>,
}

impl fmt::Debug for DaemonListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DaemonListener").finish_non_exhaustive()
    }
}

impl DaemonListener {
    #[cfg(unix)]
    fn unix(inner: tokio::net::UnixListener) -> Self {
        Self { inner }
    }

    #[cfg(windows)]
    fn create_pipe_instance(
        pipe_name: &str,
        first_pipe_instance: bool,
    ) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(first_pipe_instance)
            .create(pipe_name)
    }

    #[cfg(windows)]
    fn named_pipe(pipe_name: &str) -> io::Result<Self> {
        Ok(Self {
            pipe_name: pipe_name.to_string(),
            next: Some(Self::create_pipe_instance(pipe_name, true)?),
        })
    }

    pub async fn accept(&mut self) -> io::Result<DaemonStream> {
        #[cfg(unix)]
        {
            self.inner
                .accept()
                .await
                .map(|(stream, _)| DaemonStream::unix(stream))
        }

        #[cfg(windows)]
        {
            let server = self
                .next
                .take()
                .ok_or_else(|| io::Error::other("named pipe listener missing server instance"))?;
            if let Err(connect_error) = server.connect().await {
                match Self::create_pipe_instance(&self.pipe_name, false) {
                    Ok(next) => {
                        self.next = Some(next);
                        return Err(connect_error);
                    }
                    Err(recreate_error) => {
                        return Err(io::Error::new(
                            connect_error.kind(),
                            format!(
                                "named pipe accept failed: {connect_error}; failed to restore listener: {recreate_error}"
                            ),
                        ));
                    }
                }
            }
            self.next = Some(Self::create_pipe_instance(&self.pipe_name, false)?);
            Ok(DaemonStream::named_pipe_server(server))
        }
    }
}

#[cfg(windows)]
fn is_named_pipe_retryable(error: &io::Error) -> bool {
    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_PIPE_BUSY: i32 = 231;
    matches!(
        error.raw_os_error(),
        Some(ERROR_FILE_NOT_FOUND | ERROR_PIPE_BUSY)
    )
}

pub struct DaemonStream {
    inner: DaemonStreamInner,
}

enum DaemonStreamInner {
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
    #[cfg(windows)]
    NamedPipeClient(tokio::net::windows::named_pipe::NamedPipeClient),
    #[cfg(windows)]
    NamedPipeServer(tokio::net::windows::named_pipe::NamedPipeServer),
}

impl DaemonStream {
    #[cfg(unix)]
    fn unix(stream: tokio::net::UnixStream) -> Self {
        Self {
            inner: DaemonStreamInner::Unix(stream),
        }
    }

    #[cfg(windows)]
    fn named_pipe_client(stream: tokio::net::windows::named_pipe::NamedPipeClient) -> Self {
        Self {
            inner: DaemonStreamInner::NamedPipeClient(stream),
        }
    }

    #[cfg(windows)]
    fn named_pipe_server(stream: tokio::net::windows::named_pipe::NamedPipeServer) -> Self {
        Self {
            inner: DaemonStreamInner::NamedPipeServer(stream),
        }
    }
}

impl fmt::Debug for DaemonStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DaemonStream").finish_non_exhaustive()
    }
}

impl AsyncRead for DaemonStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut self.inner {
            #[cfg(unix)]
            DaemonStreamInner::Unix(stream) => Pin::new(stream).poll_read(cx, buf),
            #[cfg(windows)]
            DaemonStreamInner::NamedPipeClient(stream) => Pin::new(stream).poll_read(cx, buf),
            #[cfg(windows)]
            DaemonStreamInner::NamedPipeServer(stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for DaemonStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match &mut self.inner {
            #[cfg(unix)]
            DaemonStreamInner::Unix(stream) => Pin::new(stream).poll_write(cx, buf),
            #[cfg(windows)]
            DaemonStreamInner::NamedPipeClient(stream) => Pin::new(stream).poll_write(cx, buf),
            #[cfg(windows)]
            DaemonStreamInner::NamedPipeServer(stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.inner {
            #[cfg(unix)]
            DaemonStreamInner::Unix(stream) => Pin::new(stream).poll_flush(cx),
            #[cfg(windows)]
            DaemonStreamInner::NamedPipeClient(stream) => Pin::new(stream).poll_flush(cx),
            #[cfg(windows)]
            DaemonStreamInner::NamedPipeServer(stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.inner {
            #[cfg(unix)]
            DaemonStreamInner::Unix(stream) => Pin::new(stream).poll_shutdown(cx),
            #[cfg(windows)]
            DaemonStreamInner::NamedPipeClient(stream) => Pin::new(stream).poll_shutdown(cx),
            #[cfg(windows)]
            DaemonStreamInner::NamedPipeServer(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

pub enum DaemonClientStream {
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixStream),
    #[cfg(windows)]
    NamedPipe(std::fs::File),
}

impl fmt::Debug for DaemonClientStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DaemonClientStream").finish_non_exhaustive()
    }
}

impl DaemonClientStream {
    #[cfg(unix)]
    fn unix(stream: std::os::unix::net::UnixStream) -> Self {
        Self::Unix(stream)
    }

    #[cfg(windows)]
    fn named_pipe(stream: std::fs::File) -> Self {
        Self::NamedPipe(stream)
    }
}

impl Read for DaemonClientStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            #[cfg(unix)]
            Self::Unix(stream) => stream.read(buf),
            #[cfg(windows)]
            Self::NamedPipe(stream) => stream.read(buf),
        }
    }
}

impl Write for DaemonClientStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            #[cfg(unix)]
            Self::Unix(stream) => stream.write(buf),
            #[cfg(windows)]
            Self::NamedPipe(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            #[cfg(unix)]
            Self::Unix(stream) => stream.flush(),
            #[cfg(windows)]
            Self::NamedPipe(stream) => stream.flush(),
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn named_pipe_retryable_errors_cover_busy_and_missing_instances() {
        assert!(is_named_pipe_retryable(&io::Error::from_raw_os_error(2)));
        assert!(is_named_pipe_retryable(&io::Error::from_raw_os_error(231)));
        assert!(!is_named_pipe_retryable(&io::Error::from_raw_os_error(5)));
    }
}
