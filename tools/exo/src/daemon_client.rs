//! Synchronous client for communicating with the exo daemon.
//!
//! This module provides a blocking API for CLI commands to communicate with
//! the daemon. It wraps the async `ensure_daemon()` function and provides
//! simple request/response semantics over the platform daemon endpoint.
//!
//! # Usage
//!
//! ```ignore
//! use exo::daemon_client::{connect_or_spawn, send_request};
//! use exo::api::protocol::{RequestEnvelope, Op, CallParams, Address, PROTOCOL_VERSION};
//!
//! let mut stream = connect_or_spawn(&workspace_path)?;
//! let request = RequestEnvelope {
//!     protocol_version: PROTOCOL_VERSION,
//!     id: "req-1".to_string(),
//!     op: Op::Call(CallParams {
//!         address: Address::Operation { path: vec!["status".to_string()] },
//!         input: serde_json::json!({}),
//!     }),
//!     auth: None,
//!     workflow_confirmation: None,
//!     agent_id: None,
//! };
//! let response = send_request(&mut stream, &request)?;
//! ```

use crate::api::protocol::{RequestEnvelope, ResponseEnvelope};
use crate::daemon::{DaemonEnsureReport, paths_for_workspace};
use crate::daemon_diagnostics::{DaemonDiagnostics, elapsed_ms, request_op_path};
use crate::daemon_transport::DaemonClientStream;
use crate::project::Project;
use serde_json::json;
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Error, ErrorKind, Read, Write};
use std::path::Path;
use std::time::Instant;

thread_local! {
    static CLIENT_DIAGNOSTICS: RefCell<DaemonDiagnostics> = const { RefCell::new(DaemonDiagnostics::disabled()) };
}

/// Connect to the project daemon or spawn a new one.
///
/// This is a blocking wrapper around the async `ensure_daemon()` function.
/// It creates a tokio runtime to run the async code. The workspace path stays
/// part of the public CLI contract; daemon socket/PID paths are resolved from
/// the project identity.
///
/// # Errors
///
/// Returns an error if:
/// - Spawning the daemon fails
/// - The socket doesn't become available within the timeout
/// - Connection to the daemon fails
pub fn connect_or_spawn(workspace_path: &Path) -> std::io::Result<DaemonClientStream> {
    connect_or_spawn_with_report(workspace_path).map(|(stream, _report)| stream)
}

/// Connect to the project daemon or spawn one, returning the lifecycle report.
pub fn connect_or_spawn_with_report(
    workspace_path: &Path,
) -> std::io::Result<(DaemonClientStream, DaemonEnsureReport)> {
    connect_or_spawn_with_report_inner(workspace_path, None)
}

/// Connect to the project daemon using an explicitly resolved project.
pub fn connect_or_spawn_with_project_report(
    workspace_path: &Path,
    project: &Project,
) -> std::io::Result<(DaemonClientStream, DaemonEnsureReport)> {
    connect_or_spawn_with_report_inner(workspace_path, Some(project))
}

fn connect_or_spawn_with_report_inner(
    workspace_path: &Path,
    project: Option<&Project>,
) -> std::io::Result<(DaemonClientStream, DaemonEnsureReport)> {
    let paths = match project {
        Some(project) => crate::daemon::paths_for_workspace_project(workspace_path, project),
        None => paths_for_workspace(workspace_path),
    };
    let diagnostics = paths.as_ref().map_or_else(
        |_| DaemonDiagnostics::disabled(),
        |paths| DaemonDiagnostics::from_runtime_dir(&paths.runtime_dir()),
    );
    CLIENT_DIAGNOSTICS.with(|cell| *cell.borrow_mut() = diagnostics.clone());
    let start = Instant::now();
    diagnostics.record(
        "client.connect_start",
        json!({ "workspace": workspace_path.display().to_string() }),
    );

    // Create a new runtime for this blocking call
    let rt = tokio::runtime::Runtime::new()?;

    // Run the async ensure_daemon and get a transport stream.
    let outcome = match project {
        Some(project) => rt.block_on(crate::daemon::ensure_daemon_with_report_for_project(
            workspace_path,
            project,
        ))?,
        None => rt.block_on(crate::daemon::ensure_daemon_with_report(workspace_path))?,
    };
    let (tokio_stream, report) = outcome.into_parts();
    drop(tokio_stream);
    let endpoint = paths?.endpoint();
    let std_stream = endpoint.connect_blocking()?;

    diagnostics.record(
        "client.connect_end",
        json!({
            "workspace": workspace_path.display().to_string(),
            "status": "ok",
            "elapsed_ms": elapsed_ms(start.elapsed()),
        }),
    );

    Ok((std_stream, report))
}

/// Send a request to the daemon and receive a response.
///
/// Uses newline-delimited JSON (NDJSON) protocol:
/// - Request is serialized as JSON and terminated with newline
/// - Response is read as a single JSON line
///
/// # Errors
///
/// Returns an error if:
/// - Serialization of the request fails
/// - Writing to the socket fails
/// - Reading from the socket fails
/// - Deserialization of the response fails
pub fn send_request(
    stream: &mut (impl Read + Write),
    request: &RequestEnvelope,
) -> std::io::Result<ResponseEnvelope> {
    let diagnostics = client_diagnostics();
    diagnostics.record(
        "client.write_start",
        json!({ "request_id": request.id, "op_path": request_op_path(request) }),
    );
    let write_start = Instant::now();

    // Serialize request as NDJSON
    let mut data = serde_json::to_vec(request)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    data.push(b'\n');

    // Write request
    stream.write_all(&data)?;
    stream.flush()?;
    diagnostics.record(
        "client.write_end",
        json!({
            "request_id": request.id,
            "op_path": request_op_path(request),
            "elapsed_ms": elapsed_ms(write_start.elapsed()),
        }),
    );

    // Read response line
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let read_start = Instant::now();
    diagnostics.record(
        "client.read_start",
        json!({ "request_id": request.id, "op_path": request_op_path(request) }),
    );
    let bytes_read = reader.read_line(&mut line)?;
    if bytes_read == 0 {
        diagnostics.record(
            "client.read_empty_response",
            json!({ "request_id": request.id, "op_path": request_op_path(request) }),
        );
        return Err(Error::new(
            ErrorKind::UnexpectedEof,
            "daemon closed the connection without sending a response",
        ));
    }
    diagnostics.record(
        "client.read_end",
        json!({
            "request_id": request.id,
            "op_path": request_op_path(request),
            "elapsed_ms": elapsed_ms(read_start.elapsed()),
        }),
    );

    // Deserialize response
    serde_json::from_str(&line).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn client_diagnostics() -> DaemonDiagnostics {
    CLIENT_DIAGNOSTICS.with(|cell| cell.borrow().clone())
}

/// Generate a unique request ID.
pub fn generate_request_id() -> String {
    format!("cli-{}", ulid::Ulid::new().to_string().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::protocol::{Address, CallParams, Op, PROTOCOL_VERSION};

    #[test]
    fn test_generate_request_id_unique() {
        let id1 = generate_request_id();
        let id2 = generate_request_id();
        let id3 = generate_request_id();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);

        assert!(id1.starts_with("cli-"));
        assert!(id2.starts_with("cli-"));
        assert!(id3.starts_with("cli-"));
    }

    #[test]
    fn send_request_reports_empty_daemon_response_as_unexpected_eof() {
        #[derive(Default)]
        struct EmptyResponseStream {
            written: Vec<u8>,
        }

        impl Read for EmptyResponseStream {
            fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
                Ok(0)
            }
        }

        impl Write for EmptyResponseStream {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.written.extend_from_slice(buf);
                Ok(buf.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let request = RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            id: "test-empty-response".to_string(),
            op: Op::Call(CallParams {
                address: Address::Operation {
                    path: vec!["status".to_string()],
                },
                input: serde_json::json!({}),
            }),
            auth: None,
            workflow_confirmation: None,
            agent_id: None,
        };

        let mut client = EmptyResponseStream::default();
        let err = send_request(&mut client, &request).expect_err("empty response should fail");
        let written = String::from_utf8(client.written).expect("request utf8");
        assert!(written.contains("\"id\":\"test-empty-response\""));

        assert_eq!(err.kind(), ErrorKind::UnexpectedEof);
        assert!(
            err.to_string()
                .contains("daemon closed the connection without sending a response"),
            "{err}"
        );
    }
}
