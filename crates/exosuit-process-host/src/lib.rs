#![warn(unreachable_pub)]

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::Value as JsonValue;

#[derive(Debug)]
pub enum HostError {
    Io(std::io::Error),
    Json(serde_json::Error),
    MissingPipe(&'static str),
    Protocol(String),
    WorkerClosed,
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Json(error) => write!(f, "JSON error: {error}"),
            Self::MissingPipe(name) => write!(f, "worker process did not provide {name}"),
            Self::Protocol(message) => write!(f, "worker protocol error: {message}"),
            Self::WorkerClosed => write!(f, "worker process closed before replying"),
        }
    }
}

impl std::error::Error for HostError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::MissingPipe(_) | Self::Protocol(_) | Self::WorkerClosed => None,
        }
    }
}

impl From<std::io::Error> for HostError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for HostError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Clone, PartialEq)]
pub enum LineResponse {
    None,
    Reply(JsonValue),
    ReplyAndClose(JsonValue),
    Close,
}

impl LineResponse {
    pub const fn none() -> Self {
        Self::None
    }

    pub const fn reply(value: JsonValue) -> Self {
        Self::Reply(value)
    }

    pub const fn reply_and_close(value: JsonValue) -> Self {
        Self::ReplyAndClose(value)
    }

    pub const fn close() -> Self {
        Self::Close
    }
}

pub fn serve_json_lines<R, W, H>(reader: R, mut writer: W, mut handler: H) -> Result<(), HostError>
where
    R: BufRead,
    W: Write,
    H: FnMut(&str) -> LineResponse,
{
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        match handler(&line) {
            LineResponse::None => {}
            LineResponse::Reply(value) => write_response(&mut writer, &value)?,
            LineResponse::ReplyAndClose(value) => {
                write_response(&mut writer, &value)?;
                break;
            }
            LineResponse::Close => break,
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct WorkerSpec {
    program: PathBuf,
    args: Vec<String>,
    current_dir: Option<PathBuf>,
}

impl WorkerSpec {
    #[must_use]
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            current_dir: None,
        }
    }

    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    #[must_use]
    pub fn current_dir(mut self, current_dir: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(current_dir.into());
        self
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }
}

#[derive(Debug)]
pub struct JsonLineWorker {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: std::io::BufReader<ChildStdout>,
}

impl JsonLineWorker {
    #[allow(clippy::disallowed_methods)]
    pub fn spawn(spec: &WorkerSpec) -> Result<Self, HostError> {
        let mut command = Command::new(spec.program());
        command
            .args(spec.args())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(current_dir) = spec.current_dir.as_deref() {
            command.current_dir(current_dir);
        }

        let mut child = command.spawn()?;
        let stdin = child.stdin.take().ok_or(HostError::MissingPipe("stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or(HostError::MissingPipe("stdout"))?;

        Ok(Self {
            child,
            stdin: Some(stdin),
            stdout: std::io::BufReader::new(stdout),
        })
    }

    pub fn call_line(&mut self, line: &str) -> Result<Option<JsonValue>, HostError> {
        let expects_response = json_rpc_expects_response(line);
        let stdin = self.stdin.as_mut().ok_or(HostError::MissingPipe("stdin"))?;
        writeln!(stdin, "{line}")?;
        stdin.flush()?;

        if !expects_response {
            return Ok(None);
        }

        Ok(Some(read_json_line_response(&mut self.stdout)?))
    }

    pub fn process_id(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for JsonLineWorker {
    #[allow(clippy::disallowed_methods)]
    fn drop(&mut self) {
        drop(self.stdin.take());
        for _ in 0..10 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

fn json_rpc_expects_response(line: &str) -> bool {
    match serde_json::from_str::<JsonValue>(line) {
        Ok(JsonValue::Object(object)) => {
            if object.contains_key("id") {
                return true;
            }
            object
                .get("method")
                .is_none_or(|method| !method.is_string())
        }
        Ok(_) | Err(_) => true,
    }
}

fn read_json_line_response(stdout: &mut impl BufRead) -> Result<JsonValue, HostError> {
    let mut response = String::new();
    loop {
        response.clear();
        let bytes_read = stdout.read_line(&mut response)?;
        if bytes_read == 0 {
            return Err(HostError::WorkerClosed);
        }
        if response.trim().is_empty() {
            continue;
        }
        return Ok(serde_json::from_str(response.trim_end())?);
    }
}

fn write_response(writer: &mut impl Write, value: &JsonValue) -> Result<(), HostError> {
    serde_json::to_writer(&mut *writer, value)?;
    writeln!(writer)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::json;

    use super::{
        HostError, LineResponse, json_rpc_expects_response, read_json_line_response,
        serve_json_lines,
    };

    #[test]
    fn serve_json_lines_skips_empty_lines_and_writes_replies() {
        let input = Cursor::new("one\n\n two \n");
        let mut output = Vec::new();

        serve_json_lines(input, &mut output, |line| {
            LineResponse::reply(json!({ "line": line }))
        })
        .expect("serve json lines");

        let output = String::from_utf8(output).expect("utf8 output");
        let lines = output.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], r#"{"line":"one"}"#);
        assert_eq!(lines[1], r#"{"line":" two "}"#);
    }

    #[test]
    fn serve_json_lines_stops_after_reply_and_close() {
        let input = Cursor::new("one\ntwo\n");
        let mut output = Vec::new();

        serve_json_lines(input, &mut output, |line| {
            LineResponse::reply_and_close(json!({ "line": line }))
        })
        .expect("serve json lines");

        let output = String::from_utf8(output).expect("utf8 output");
        assert_eq!(
            output.lines().collect::<Vec<_>>(),
            vec![r#"{"line":"one"}"#]
        );
    }

    #[test]
    fn json_rpc_expects_response_for_requests_and_protocol_errors() {
        assert!(json_rpc_expects_response(
            r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#
        ));
        assert!(!json_rpc_expects_response(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
        ));
        assert!(json_rpc_expects_response("not json"));
        assert!(json_rpc_expects_response("[]"));
        assert!(json_rpc_expects_response("{}"));
        assert!(json_rpc_expects_response(r#"{"jsonrpc":"2.0","id":null}"#));
        assert!(json_rpc_expects_response(
            r#"{"jsonrpc":"2.0","id":null,"method":"notifications/initialized"}"#
        ));
    }

    #[test]
    fn read_json_line_response_errors_when_worker_closes_before_reply() {
        let mut output = Cursor::new("");
        let error = read_json_line_response(&mut output).expect_err("worker closed error");
        assert!(matches!(error, HostError::WorkerClosed));
    }

    #[test]
    fn read_json_line_response_skips_blank_lines_before_reply() {
        let mut output = Cursor::new("\n  \n{\"ok\":true}\n");
        let value = read_json_line_response(&mut output).expect("json response");
        assert_eq!(value, json!({ "ok": true }));
    }
}
