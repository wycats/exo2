use chrono::{DateTime, Utc};
use serde::Serialize;
use std::io::{self, Write};
use std::sync::Mutex;

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(dead_code)]
pub enum JsonlEvent {
    LaneStarted {
        lane: String,
        check_count: usize,
        parallel: bool,
        timestamp: DateTime<Utc>,
    },
    CheckEnqueued {
        check_id: String,
        lane: String,
        index: usize,
        label: String,
        timestamp: DateTime<Utc>,
    },
    CheckStarted {
        check_id: String,
        lane: String,
        index: usize,
        command: String,
        working_dir: Option<String>,
        /// The glob filters configured for this check.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        filters: Vec<String>,
        /// The files that matched the filters.
        #[serde(skip_serializing_if = "Option::is_none")]
        matched_files: Option<Vec<String>>,
        timestamp: DateTime<Utc>,
    },
    CheckOutput {
        check_id: String,
        lane: String,
        stream: OutputStream,
        data: String,
        timestamp: DateTime<Utc>,
    },
    CheckCompleted {
        check_id: String,
        lane: String,
        status: CheckStatus,
        exit_code: Option<i32>,
        duration_ms: u64,
        output_bytes: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        skip_reason: Option<SkipReason>,
        timestamp: DateTime<Utc>,
    },
    /// Restage failed after a successful mutate check.
    /// This is a serious error — the check itself passed, but
    /// re-staging the fixed files failed (e.g., containment violation).
    RestageFailed {
        check_id: String,
        lane: String,
        error: String,
        timestamp: DateTime<Utc>,
    },
    LaneCompleted {
        lane: String,
        status: LaneStatus,
        passed: usize,
        failed: usize,
        skipped: usize,
        duration_ms: u64,
        timestamp: DateTime<Utc>,
    },
    Summary {
        protocol_version: u8,
        lane: String,
        status: LaneStatus,
        checks: Vec<CheckSummary>,
        duration_ms: u64,
        timestamp: DateTime<Utc>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DiscoveryItem {
    Suite {
        id: String,
        label: String,
        checks: Vec<String>,
    },
    Check {
        id: String,
        label: String,
        command: String,
        lane: String,
        category: String,
        filters: Vec<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum CheckStatus {
    Success,
    Failure,
    Timeout,
    Cancelled,
    Skipped,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// Check had file filters but no staged files matched.
    NoMatchingFiles,
    /// A prior check failed and fail-fast stopped execution.
    FailFast {
        /// The check_id of the check that failed.
        failed_check: String,
    },
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LaneStatus {
    Success,
    Failure,
}

#[derive(Debug, Serialize)]
pub struct CheckSummary {
    pub id: String,
    pub status: CheckStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<SkipReason>,
}

pub struct JsonlEmitter {
    writer: Mutex<Box<dyn Write + Send>>,
}

impl JsonlEmitter {
    pub fn stdout() -> Self {
        Self {
            writer: Mutex::new(Box::new(io::stdout())),
        }
    }

    pub fn emit(&self, event: &JsonlEvent) -> io::Result<()> {
        let json = serde_json::to_string(event).map_err(io::Error::other)?;
        let mut writer = self.writer.lock().unwrap();
        writeln!(writer, "{}", json)?;
        writer.flush()
    }
}
