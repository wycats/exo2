//! Opt-in daemon diagnostics written as NDJSON.

use fs2::FileExt;
use serde_json::{Value as JsonValue, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::api::protocol::{Address, Effect, Op, RequestEnvelope, ResponseEnvelope, Status};

pub const ENABLED_ENV: &str = "EXO_DAEMON_DIAGNOSTICS";
pub const PATH_ENV: &str = "EXO_DAEMON_DIAG_PATH";
pub const SLOW_MS_ENV: &str = "EXO_DAEMON_DIAG_SLOW_MS";

#[derive(Debug, Clone)]
pub struct DaemonDiagnosticsConfig {
    pub enabled: bool,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct DaemonDiagnostics {
    path: Option<PathBuf>,
}

static WRITE_LOCK: Mutex<()> = Mutex::new(());
const FILE_LOCK_RETRY_TIMEOUT: Duration = Duration::from_millis(250);
const FILE_LOCK_RETRY_DELAY: Duration = Duration::from_millis(10);

impl DaemonDiagnostics {
    pub fn from_runtime_dir(runtime_dir: &Path) -> Self {
        if std::env::var_os(ENABLED_ENV).is_none() {
            return Self { path: None };
        }

        let path = std::env::var_os(PATH_ENV).map_or_else(
            || runtime_dir.join("daemon-diagnostics.ndjson"),
            PathBuf::from,
        );

        Self { path: Some(path) }
    }

    pub fn from_runtime_dir_with_config(
        runtime_dir: &Path,
        config: Option<&DaemonDiagnosticsConfig>,
    ) -> Self {
        let Some(config) = config else {
            return Self::from_runtime_dir(runtime_dir);
        };
        if !config.enabled {
            return Self { path: None };
        }

        let path = config
            .path
            .clone()
            .unwrap_or_else(|| runtime_dir.join("daemon-diagnostics.ndjson"));
        Self { path: Some(path) }
    }

    pub const fn disabled() -> Self {
        Self { path: None }
    }

    pub const fn is_active(&self) -> bool {
        self.path.is_some()
    }

    pub fn record(&self, event: &str, fields: JsonValue) {
        let Some(path) = &self.path else {
            return;
        };

        let path = path.clone();
        let event = event.to_string();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(move || write_record(&path, &event, fields));
        } else {
            std::thread::spawn(move || write_record(&path, &event, fields));
        }
    }
}

fn write_record(path: &Path, event: &str, fields: JsonValue) {
    let Ok(_guard) = WRITE_LOCK.lock() else {
        return;
    };

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut obj = match fields {
        JsonValue::Object(map) => map,
        value => {
            let mut map = serde_json::Map::new();
            map.insert("fields".to_string(), value);
            map
        }
    };
    obj.insert("ts".to_string(), json!(chrono::Utc::now().to_rfc3339()));
    obj.insert("event".to_string(), json!(event));

    let Ok(line) = serde_json::to_string(&JsonValue::Object(obj)) else {
        return;
    };

    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    if !try_lock_diagnostics_file(&file) {
        return;
    }
    let mut bytes = line.into_bytes();
    bytes.push(b'\n');
    let _ = file.write_all(&bytes);
    let _ = file.unlock();
}

fn try_lock_diagnostics_file(file: &std::fs::File) -> bool {
    let start = Instant::now();
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return true,
            Err(_) if start.elapsed() >= FILE_LOCK_RETRY_TIMEOUT => return false,
            Err(_) => std::thread::sleep(FILE_LOCK_RETRY_DELAY),
        }
    }
}

pub fn enabled_env_vars() -> impl Iterator<Item = (&'static str, std::ffi::OsString)> {
    [ENABLED_ENV, PATH_ENV, SLOW_MS_ENV]
        .into_iter()
        .filter_map(|key| std::env::var_os(key).map(|value| (key, value)))
}

pub fn request_op_path(request: &RequestEnvelope) -> String {
    match &request.op {
        Op::Help(params) => format!("help:{}", address_path(&params.address)),
        Op::List(params) => format!("list:{}", address_path(&params.address)),
        Op::Call(params) => address_path(&params.address),
        Op::Preview(params) => format!("preview:{}", address_path(&params.address)),
    }
}

pub fn address_path(address: &Address) -> String {
    match address {
        Address::Root => "root".to_string(),
        Address::Namespace { path } | Address::Operation { path } => path.join("."),
    }
}

pub const fn response_status(response: &ResponseEnvelope) -> &'static str {
    match response.status {
        Status::Ok => "ok",
        Status::Error => "error",
        Status::NeedsInput => "needs_input",
        Status::ConfirmRequired => "confirm_required",
    }
}

pub const fn effect_name(effect: Effect) -> &'static str {
    match effect {
        Effect::Pure => "pure",
        Effect::Write => "write",
        Effect::Exec => "exec",
    }
}

pub fn elapsed_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::thread;

    #[test]
    fn write_record_returns_without_writing_when_file_lock_is_unavailable() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("daemon-diagnostics.ndjson");
        std::fs::write(&path, "").expect("create diagnostics file");

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .expect("open diagnostics file");
        file.lock_exclusive().expect("lock diagnostics file");

        write_record(&path, "blocked.write", json!({ "request": "held-lock" }));

        file.unlock().expect("unlock diagnostics file");
        let contents = std::fs::read_to_string(&path).expect("read diagnostics file");
        assert!(
            contents.is_empty(),
            "diagnostics write should be skipped while the file lock is unavailable: {contents:?}"
        );
    }

    #[test]
    fn write_record_retries_when_file_lock_is_temporarily_unavailable() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("daemon-diagnostics.ndjson");
        std::fs::write(&path, "").expect("create diagnostics file");

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .expect("open diagnostics file");
        file.lock_exclusive().expect("lock diagnostics file");

        let unlocker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            file.unlock().expect("unlock diagnostics file");
        });

        write_record(&path, "retried.write", json!({ "request": "brief-lock" }));
        unlocker.join().expect("join unlocker");

        let contents = std::fs::read_to_string(&path).expect("read diagnostics file");
        let lines = contents.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1, "expected retried diagnostics write");

        let event: JsonValue = serde_json::from_str(lines[0]).expect("parse diagnostics line");
        assert_eq!(event["event"], "retried.write");
        assert_eq!(event["request"], "brief-lock");
    }

    #[test]
    fn write_record_appends_valid_ndjson_after_lock_is_available() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("daemon-diagnostics.ndjson");

        write_record(&path, "request.done", json!({ "status": "ok" }));

        let contents = std::fs::read_to_string(&path).expect("read diagnostics file");
        let lines = contents.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1, "expected one diagnostics line");

        let event: JsonValue = serde_json::from_str(lines[0]).expect("parse diagnostics line");
        assert_eq!(event["event"], "request.done");
        assert_eq!(event["status"], "ok");
        assert!(
            event.get("ts").is_some(),
            "diagnostics event should include a timestamp"
        );
    }
}
