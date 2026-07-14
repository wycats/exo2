use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use console::Term;
use indicatif::MultiProgress;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::ColorMode;
use crate::OutputFormat;
use crate::config::{
    CheckCategory, CheckRefV3, CheckV3, ConfigV3, DefaultsV3, ExecutionContext, HookType,
    RunnerConfig, get_check_table, get_lane_table, lane_override_table, parse_runner_config,
    read_hooks_doc,
};
use crate::fileset::{FilesetScope, compute_fileset, rebase_files_for_cwd, substitute_files};
use crate::filter::filter_files;
use crate::hooks::git_repo_root;
use crate::jsonl::{
    CheckStatus, CheckSummary, JsonlEmitter, JsonlEvent, LaneStatus, OutputStream, SkipReason,
};
use crate::lane::{colors_enabled, show_lane_listing};
use crate::pipe_runner;
use crate::resolve_check_command_parts;
use crate::shell::shell_command_parts;
use crate::terminal::{
    TerminalConfig, compact_progress_indicator, format_lane_summary, format_result_line,
};
use crate::validate_hooks_doc;
use crate::{CheckProgressGroup, OutputBuffer, OutputMode, spawn_check};
use toml_edit::{Item, Table};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestagePolicy {
    Off,
    Auto,
}

impl RestagePolicy {
    /// Returns `Auto` if the condition is true, `Off` otherwise.
    fn auto_if(condition: bool) -> Self {
        if condition {
            RestagePolicy::Auto
        } else {
            RestagePolicy::Off
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContainmentPolicy {
    Off,
    Warn,
    Fail,
}

#[derive(Debug, Clone)]
struct CheckPlan {
    id: String,
    label: String,
    parts: Vec<String>,
    workdir: PathBuf,
    category: CheckCategory,
    restage: RestagePolicy,
    containment: ContainmentPolicy,
    tool: Option<ToolPlan>,
    skipped: bool,
    /// The glob filters configured for this check (empty = matches all files).
    filters: Vec<String>,
    /// The files that matched the filters (None = no file scoping).
    matched_files: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct ToolPlan {
    id: String,
    #[allow(dead_code)]
    label: String,
    tool_address: Vec<String>,
    files: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct CheckResult {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    duration: Duration,
}

#[derive(Debug, Clone)]
struct ToolResult {
    success: bool,
    message: Option<String>,
    duration: Duration,
}

/// Machine channel request envelope
#[derive(Debug, Serialize)]
struct ToolRequest {
    protocol_version: u8,
    id: String,
    op: ToolOp,
}

#[derive(Debug, Serialize)]
struct ToolOp {
    kind: String,
    params: ToolParams,
}

#[derive(Debug, Serialize)]
struct ToolParams {
    address: ToolAddress,
    input: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct ToolAddress {
    kind: String,
    path: Vec<String>,
}

/// Machine channel response envelope
#[derive(Debug, Deserialize)]
struct ToolResponse {
    #[allow(dead_code)]
    protocol_version: u8,
    #[allow(dead_code)]
    id: String,
    status: String,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<ToolError>,
}

#[derive(Debug, Deserialize)]
struct ToolError {
    message: String,
}

/// Invoke a tool via the exo json server subprocess
fn invoke_tool(tool: &ToolPlan) -> Result<ToolResult> {
    let start = Instant::now();

    // Build the request
    let mut input = serde_json::Map::new();
    if let Some(files) = &tool.files {
        input.insert("targets".to_string(), serde_json::json!({ "paths": files }));
    }

    let request = ToolRequest {
        protocol_version: 1,
        id: tool.id.clone(),
        op: ToolOp {
            kind: "call".to_string(),
            params: ToolParams {
                address: ToolAddress {
                    kind: "operation".to_string(),
                    path: tool.tool_address.clone(),
                },
                input: serde_json::Value::Object(input),
            },
        },
    };

    let request_json = serde_json::to_string(&request)?;

    // Spawn exo json server
    let mut child = Command::new("cargo")
        .args(["exo", "json", "server"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn exo json server")?;

    // Write request to stdin
    if let Some(mut stdin) = child.stdin.take() {
        writeln!(stdin, "{}", request_json)?;
    }

    // Read response from stdout
    let stdout = child.stdout.take().context("failed to capture stdout")?;
    let reader = BufReader::new(stdout);

    let mut response_line = None;
    for line in reader.lines() {
        let line = line?;
        if !line.is_empty() {
            response_line = Some(line);
            break;
        }
    }

    // Kill the subprocess (it's a one-shot for now)
    let _ = child.kill();
    let _ = child.wait();

    let duration = start.elapsed();

    let Some(response_json) = response_line else {
        return Ok(ToolResult {
            success: false,
            message: Some("no response from tool".to_string()),
            duration,
        });
    };

    let response: ToolResponse =
        serde_json::from_str(&response_json).context("failed to parse tool response")?;

    if response.status == "ok" {
        Ok(ToolResult {
            success: true,
            message: None,
            duration,
        })
    } else {
        let message = response
            .error
            .map(|e| e.message)
            .or_else(|| {
                response
                    .result
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
            })
            .unwrap_or_else(|| "tool invocation failed".to_string());
        Ok(ToolResult {
            success: false,
            message: Some(message),
            duration,
        })
    }
}

fn print_grouped_output(check: &CheckPlan, result: &CheckResult) {
    println!("==> {} ({})", check.id, check.label);
    if !result.stdout.is_empty() {
        print!("{}", String::from_utf8_lossy(&result.stdout));
        if !result.stdout.ends_with(b"\n") {
            println!();
        }
    }
    if !result.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&result.stderr));
        if !result.stderr.ends_with(b"\n") {
            eprintln!();
        }
    }
}

fn print_compact_status_line(
    check: &CheckPlan,
    ok: bool,
    duration: Duration,
    use_color: bool,
    label_padding: usize,
) {
    println!(
        "{}",
        format_result_line(ok, &check.label, duration, label_padding, use_color)
    );
}

fn print_compact_failure_details(_check: &CheckPlan, result: &CheckResult) {
    if !result.stdout.is_empty() {
        println!("--- stdout ---");
        print!("{}", String::from_utf8_lossy(&result.stdout));
        if !result.stdout.ends_with(b"\n") {
            println!();
        }
    }
    if !result.stderr.is_empty() {
        println!("--- stderr ---");
        print!("{}", String::from_utf8_lossy(&result.stderr));
        if !result.stderr.ends_with(b"\n") {
            println!();
        }
    }
}

/// Run a command and capture output without streaming.
/// Used for parallel execution in non-TTY mode.
fn run_command_capture(work_dir: &Path, program: &str, args: &[String]) -> Result<CheckResult> {
    // Use pipe runner directly for capture mode (no streaming, no PTY)
    let runner_result = pipe_runner::spawn_streaming(
        program, args, work_dir, None,  // No output buffer needed
        false, // No streaming
    )?;

    Ok(CheckResult {
        status: runner_result.status,
        stdout: runner_result.stdout,
        stderr: runner_result.stderr,
        duration: runner_result.duration,
    })
}

/// Run a command, capture output, and emit `CheckOutput` JSONL events as chunks arrive.
/// Used in JSONL mode so the Test Explorer can display output in real-time.
fn run_command_capture_jsonl(
    work_dir: &Path,
    program: &str,
    args: &[String],
    check_id: &str,
    lane: &str,
    emitter: &JsonlEmitter,
) -> Result<CheckResult> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;

    let start = Instant::now();

    let mut child = Command::new(program)
        .args(args)
        .current_dir(work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn '{program}'"))?;

    let stdout_pipe = child.stdout.take().expect("stdout was piped");
    let stderr_pipe = child.stderr.take().expect("stderr was piped");

    let (tx, rx) = mpsc::channel::<(OutputStream, Vec<u8>)>();
    let tx_stdout = tx.clone();
    let tx_stderr = tx;

    // Stdout reader thread
    let stdout_handle = thread::spawn(move || {
        let mut reader = stdout_pipe;
        let mut buf = [0u8; 4096];
        let mut all = Vec::new();
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = buf[..n].to_vec();
                    all.extend_from_slice(&chunk);
                    let _ = tx_stdout.send((OutputStream::Stdout, chunk));
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        all
    });

    // Stderr reader thread
    let stderr_handle = thread::spawn(move || {
        let mut reader = stderr_pipe;
        let mut buf = [0u8; 4096];
        let mut all = Vec::new();
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = buf[..n].to_vec();
                    all.extend_from_slice(&chunk);
                    let _ = tx_stderr.send((OutputStream::Stderr, chunk));
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        all
    });

    // Drain rx: emit CheckOutput events for each chunk
    for (stream, chunk) in rx {
        let text = String::from_utf8_lossy(&chunk).into_owned();
        if !text.is_empty() {
            let _ = emit_jsonl(
                emitter,
                JsonlEvent::CheckOutput {
                    check_id: check_id.to_string(),
                    lane: lane.to_string(),
                    stream,
                    data: text,
                    timestamp: Utc::now(),
                },
            );
        }
    }

    let stdout_bytes = stdout_handle.join().unwrap_or_default();
    let stderr_bytes = stderr_handle.join().unwrap_or_default();

    let status = child
        .wait()
        .with_context(|| format!("failed waiting for '{program}'"))?;

    Ok(CheckResult {
        status,
        stdout: stdout_bytes,
        stderr: stderr_bytes,
        duration: start.elapsed(),
    })
}

/// Run a command with streaming output (lines printed as they arrive).
/// Used for sequential checks when we want real-time feedback.
///
/// If `output_buffer` is provided, raw bytes are fed to it for VTE parsing
/// (used in parallel mode with progress groups). When `None`, behavior is
/// unchanged from before.
///
/// This function uses hybrid PTY/pipe dispatch:
/// - On Unix TTYs: uses PTY for native colors (stdout/stderr merged)
/// - On Windows, CI, or machine output: uses pipes (separate streams)
fn run_command_streaming(
    work_dir: &Path,
    program: &str,
    args: &[String],
    stream_output: bool,
    output_buffer: Option<OutputBuffer>,
    force_pipes: bool,
    term_width: u16,
) -> Result<CheckResult> {
    // Debug: log buffer identity at run_command_streaming entry
    #[cfg(debug_assertions)]
    if std::env::var("EXOHOOK_DEBUG_SPAWN").is_ok()
        && let Some(ref b) = output_buffer
    {
        eprintln!("[RUN_STREAMING] arc={:x}", b.arc_id());
    }

    // Use the hybrid dispatch from check_runner
    let runner_result = spawn_check(
        program,
        args,
        work_dir,
        OutputMode::Human, // Human output mode for TTY detection
        output_buffer,
        stream_output,
        force_pipes,
        term_width,
    )?;

    // Convert runner result to local CheckResult
    // For PTY mode, stdout contains merged output; stderr is empty
    // For pipe mode, both are populated
    Ok(CheckResult {
        status: runner_result.status,
        stdout: if runner_result.used_pty {
            runner_result.output
        } else {
            runner_result.stdout
        },
        stderr: runner_result.stderr,
        duration: runner_result.duration,
    })
}

fn duration_ms(duration: Duration) -> u64 {
    let ms = duration.as_millis();
    u64::try_from(ms).unwrap_or(u64::MAX)
}

fn output_preview(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    let combined = [stdout, stderr].concat();
    if combined.is_empty() {
        return None;
    }

    let preview: String = String::from_utf8_lossy(&combined)
        .chars()
        .take(500)
        .collect();
    Some(preview)
}

fn plan_command_string(plan: &CheckPlan) -> String {
    if let Some(tool) = &plan.tool {
        return format!("tool:{}", tool.tool_address.join("."));
    }

    plan.parts.join(" ")
}

fn emit_jsonl(emitter: &JsonlEmitter, event: JsonlEvent) -> Result<()> {
    emitter.emit(&event).map_err(|e| anyhow!(e))
}

fn report_empty_category_selection(
    format: OutputFormat,
    category: Option<CheckCategory>,
    workflow_name: &str,
    selected_check_count: usize,
) -> bool {
    let Some(category) = category.filter(|_| selected_check_count == 0) else {
        return false;
    };

    if format != OutputFormat::Jsonl {
        let category = match category {
            CheckCategory::Observe => "observe",
            CheckCategory::Mutate => "mutate",
        };
        println!("No checks matched --category {category} in workflow '{workflow_name}'.");
    }

    true
}

#[allow(clippy::too_many_arguments)]
fn run_check_plans_jsonl(
    lane: &str,
    plans: Vec<CheckPlan>,
    lane_parallel: bool,
    scope_base: Option<String>,
    repo_root: PathBuf,
    emitter: Arc<JsonlEmitter>,
) -> Result<()> {
    let lane_start = Instant::now();
    let lane_name = lane.to_string();
    emit_jsonl(
        &emitter,
        JsonlEvent::LaneStarted {
            lane: lane_name.clone(),
            check_count: plans.len(),
            parallel: lane_parallel,
            timestamp: Utc::now(),
        },
    )?;

    // Emit CheckEnqueued for every check before any start running.
    // This lets the Test Explorer show all pending checks with a clock icon.
    for (idx, plan) in plans.iter().enumerate() {
        emit_jsonl(
            &emitter,
            JsonlEvent::CheckEnqueued {
                check_id: plan.id.clone(),
                lane: lane_name.clone(),
                index: idx,
                label: plan.label.clone(),
                timestamp: Utc::now(),
            },
        )?;
    }

    let is_staged_scope = scope_base.as_deref() == Some("staged");
    let (parallel_indices, sequential_indices): (Vec<usize>, Vec<usize>) =
        if lane_parallel && is_staged_scope {
            let mut par = Vec::new();
            let mut seq = Vec::new();
            for (idx, plan) in plans.iter().enumerate() {
                if plan.category == CheckCategory::Mutate && plan.restage == RestagePolicy::Auto {
                    seq.push(idx);
                } else {
                    par.push(idx);
                }
            }
            (par, seq)
        } else if lane_parallel {
            ((0..plans.len()).collect(), Vec::new())
        } else {
            (Vec::new(), (0..plans.len()).collect())
        };

    let has_parallel = !parallel_indices.is_empty();
    let _has_sequential = !sequential_indices.is_empty();

    let mut results: Vec<Option<Result<CheckSummary>>> = (0..plans.len()).map(|_| None).collect();

    if has_parallel {
        let mut handles = Vec::with_capacity(parallel_indices.len());
        for &original_idx in &parallel_indices {
            let plan = plans[original_idx].clone();
            let emitter = emitter.clone();
            let lane_name = lane_name.clone();
            handles.push((
                original_idx,
                thread::spawn(move || run_jsonl_check(&lane_name, original_idx, plan, &emitter)),
            ));
        }

        for (original_idx, handle) in handles {
            let res = match handle.join() {
                Ok(v) => v,
                Err(_) => Err(anyhow!("check thread panicked")),
            };
            results[original_idx] = Some(res);
        }
    }

    for &original_idx in &sequential_indices {
        let plan = plans[original_idx].clone();
        let before_unstaged = if plan.restage == RestagePolicy::Auto {
            git_lines(&repo_root, &["diff", "--name-only"]).unwrap_or_default()
        } else {
            Vec::new()
        };

        let res = run_jsonl_check(lane, original_idx, plan.clone(), &emitter);

        if let Ok(ref ok) = res
            && plan.restage == RestagePolicy::Auto
            && ok.status == CheckStatus::Success
        {
            let after_unstaged =
                git_lines(&repo_root, &["diff", "--name-only"]).unwrap_or_default();
            if let Err(e) = apply_restage_if_needed(
                &repo_root,
                scope_base.as_deref(),
                &plan,
                &before_unstaged,
                &after_unstaged,
            ) {
                // Emit restage_failed — this is a serious error, not a normal check failure.
                // The check itself passed, but re-staging the fixed files failed.
                let _ = emit_jsonl(
                    &emitter,
                    JsonlEvent::RestageFailed {
                        check_id: plan.id.clone(),
                        lane: lane.to_string(),
                        error: e.to_string(),
                        timestamp: Utc::now(),
                    },
                );
                results[original_idx] = Some(Err(e));
                break;
            }
        }

        let should_stop = match &res {
            Ok(ok) => ok.status != CheckStatus::Success,
            Err(_) => true,
        };
        results[original_idx] = Some(res);

        if should_stop {
            break;
        }
    }

    // Emit fail-fast skipped events for checks that never ran
    let failed_check_id = results.iter().enumerate().find_map(|(idx, opt)| match opt {
        Some(Ok(s)) if s.status == CheckStatus::Failure => Some(plans[idx].id.clone()),
        Some(Err(_)) => Some(plans[idx].id.clone()),
        _ => None,
    });

    for (idx, slot) in results.iter().enumerate() {
        if slot.is_none() && !plans[idx].skipped {
            // This check was never run due to fail-fast
            if let Some(ref failed_id) = failed_check_id {
                let _ = emit_jsonl(
                    &emitter,
                    JsonlEvent::CheckStarted {
                        check_id: plans[idx].id.clone(),
                        lane: lane.to_string(),
                        index: idx,
                        command: plan_command_string(&plans[idx]),
                        working_dir: Some(plans[idx].workdir.to_string_lossy().to_string()),
                        filters: plans[idx].filters.clone(),
                        matched_files: plans[idx].matched_files.clone(),
                        timestamp: Utc::now(),
                    },
                );
                let _ = emit_jsonl(
                    &emitter,
                    JsonlEvent::CheckCompleted {
                        check_id: plans[idx].id.clone(),
                        lane: lane.to_string(),
                        status: CheckStatus::Skipped,
                        exit_code: None,
                        duration_ms: 0,
                        output_bytes: 0,
                        skip_reason: Some(SkipReason::FailFast {
                            failed_check: failed_id.clone(),
                        }),
                        timestamp: Utc::now(),
                    },
                );
            }
        }
    }

    let ran_results: Vec<(usize, Result<CheckSummary>)> = results
        .into_iter()
        .enumerate()
        .filter_map(|(idx, opt)| opt.map(|r| (idx, r)))
        .collect();

    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let mut summaries = Vec::with_capacity(ran_results.len());
    let mut first_err: Option<anyhow::Error> = None;

    for (idx, res) in ran_results {
        let plan = &plans[idx];
        match res {
            Ok(summary) => {
                match summary.status {
                    CheckStatus::Success => passed += 1,
                    CheckStatus::Failure => failed += 1,
                    CheckStatus::Skipped => skipped += 1,
                    _ => failed += 1,
                }
                summaries.push(summary);
            }
            Err(e) => {
                failed += 1;
                if first_err.is_none() {
                    first_err = Some(anyhow!(e.to_string()));
                }
                summaries.push(CheckSummary {
                    id: plan.id.clone(),
                    status: CheckStatus::Failure,
                    exit_code: None,
                    duration_ms: 0,
                    output_preview: Some(e.to_string()),
                    skip_reason: None,
                });
            }
        }
    }

    let lane_status = if failed == 0 {
        LaneStatus::Success
    } else {
        LaneStatus::Failure
    };

    emit_jsonl(
        &emitter,
        JsonlEvent::LaneCompleted {
            lane: lane.to_string(),
            status: lane_status,
            passed,
            failed,
            skipped,
            duration_ms: duration_ms(lane_start.elapsed()),
            timestamp: Utc::now(),
        },
    )?;

    emit_jsonl(
        &emitter,
        JsonlEvent::Summary {
            protocol_version: 1,
            lane: lane.to_string(),
            status: lane_status,
            checks: summaries,
            duration_ms: duration_ms(lane_start.elapsed()),
            timestamp: Utc::now(),
        },
    )?;

    if failed > 0 {
        if let Some(e) = first_err {
            return Err(e);
        }
        return Err(anyhow!("lane '{lane}' failed"));
    }

    Ok(())
}

fn run_jsonl_check(
    lane: &str,
    index: usize,
    plan: CheckPlan,
    emitter: &JsonlEmitter,
) -> Result<CheckSummary> {
    let command = plan_command_string(&plan);
    let working_dir = Some(plan.workdir.to_string_lossy().to_string());
    emit_jsonl(
        emitter,
        JsonlEvent::CheckStarted {
            check_id: plan.id.clone(),
            lane: lane.to_string(),
            index,
            command,
            working_dir,
            filters: plan.filters.clone(),
            matched_files: plan.matched_files.clone(),
            timestamp: Utc::now(),
        },
    )?;

    if plan.skipped {
        emit_jsonl(
            emitter,
            JsonlEvent::CheckCompleted {
                check_id: plan.id.clone(),
                lane: lane.to_string(),
                status: CheckStatus::Skipped,
                exit_code: None,
                duration_ms: 0,
                output_bytes: 0,
                skip_reason: Some(SkipReason::NoMatchingFiles),
                timestamp: Utc::now(),
            },
        )?;

        return Ok(CheckSummary {
            id: plan.id,
            status: CheckStatus::Skipped,
            exit_code: None,
            duration_ms: 0,
            output_preview: None,
            skip_reason: Some(SkipReason::NoMatchingFiles),
        });
    }

    let start = Instant::now();
    let result = if let Some(tool) = &plan.tool {
        match invoke_tool(tool) {
            Ok(tool_result) => {
                let status = if tool_result.success {
                    CheckStatus::Success
                } else {
                    CheckStatus::Failure
                };

                let preview = if tool_result.success {
                    None
                } else {
                    tool_result.message.clone()
                };

                let output_bytes = preview.as_ref().map(|s| s.len()).unwrap_or(0);

                Ok((
                    CheckSummary {
                        id: plan.id.clone(),
                        status,
                        exit_code: None,
                        duration_ms: duration_ms(tool_result.duration),
                        output_preview: preview,
                        skip_reason: None,
                    },
                    output_bytes,
                ))
            }
            Err(e) => Err(e),
        }
    } else {
        let (program, args) = plan
            .parts
            .split_first()
            .ok_or_else(|| anyhow!("check '{}' argv must not be empty", plan.id))?;
        let res = run_command_capture_jsonl(&plan.workdir, program, args, &plan.id, lane, emitter)?;
        let status = if res.status.success() {
            CheckStatus::Success
        } else {
            CheckStatus::Failure
        };
        let preview = if status == CheckStatus::Failure {
            output_preview(&res.stdout, &res.stderr)
        } else {
            None
        };
        let output_bytes = res.stdout.len() + res.stderr.len();
        Ok((
            CheckSummary {
                id: plan.id.clone(),
                status,
                exit_code: res.status.code(),
                duration_ms: duration_ms(res.duration),
                output_preview: preview,
                skip_reason: None,
            },
            output_bytes,
        ))
    };

    match result {
        Ok((summary, output_bytes)) => {
            emit_jsonl(
                emitter,
                JsonlEvent::CheckCompleted {
                    check_id: summary.id.clone(),
                    lane: lane.to_string(),
                    status: summary.status,
                    exit_code: summary.exit_code,
                    duration_ms: summary.duration_ms,
                    output_bytes,
                    skip_reason: None,
                    timestamp: Utc::now(),
                },
            )?;
            Ok(summary)
        }
        Err(e) => {
            emit_jsonl(
                emitter,
                JsonlEvent::CheckCompleted {
                    check_id: plan.id.clone(),
                    lane: lane.to_string(),
                    status: CheckStatus::Failure,
                    exit_code: None,
                    duration_ms: duration_ms(start.elapsed()),
                    output_bytes: 0,
                    skip_reason: None,
                    timestamp: Utc::now(),
                },
            )?;
            Err(e)
        }
    }
}

fn git_lines(repo_root: &Path, args: &[&str]) -> Result<Vec<String>> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !out.status.success() {
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect())
}

fn git_add(repo_root: &Path, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }

    let mut cmd = Command::new("git");
    cmd.current_dir(repo_root).arg("add").arg("--");
    for p in paths {
        cmd.arg(p);
    }
    let out = cmd.output().context("failed to run git add")?;
    if !out.status.success() {
        return Err(anyhow!(
            "git add failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

fn apply_restage_if_needed(
    repo_root: &Path,
    scope_base: Option<&str>,
    check: &CheckPlan,
    before_unstaged: &[String],
    after_unstaged: &[String],
) -> Result<()> {
    if check.restage != RestagePolicy::Auto {
        return Ok(());
    }

    if scope_base != Some("staged") {
        eprintln!(
            "warning: restage=auto ignored for check '{}' (only supported for scope.base=staged)",
            check.id
        );
        return Ok(());
    }

    let lane_paths = git_lines(repo_root, &["diff", "--name-only", "--cached"])?;
    let lane_set: std::collections::HashSet<&str> = lane_paths.iter().map(|s| s.as_str()).collect();

    let before_set: std::collections::HashSet<&str> =
        before_unstaged.iter().map(|s| s.as_str()).collect();
    let after_set: std::collections::HashSet<&str> =
        after_unstaged.iter().map(|s| s.as_str()).collect();

    // Safety: never stage unstaged changes to lane-scoped files.
    let unsafe_unstaged: Vec<&str> = before_set
        .iter()
        .copied()
        .filter(|p| lane_set.contains(p))
        .collect();
    if !unsafe_unstaged.is_empty() {
        return Err(anyhow!(
            "mutate+restage requires lane-scoped files to have no unstaged changes; found: {}",
            unsafe_unstaged.join(", ")
        ));
    }

    // Containment: detect newly-introduced unstaged changes outside lane scope.
    let newly_dirty: Vec<&str> = after_set.difference(&before_set).copied().collect();

    let outside: Vec<&str> = newly_dirty
        .iter()
        .copied()
        .filter(|p| !lane_set.contains(p))
        .collect();

    if !outside.is_empty() {
        match check.containment {
            ContainmentPolicy::Off => {}
            ContainmentPolicy::Warn => {
                eprintln!(
                    "warning: mutate check '{}' modified files outside lane scope: {}",
                    check.id,
                    outside.join(", ")
                );
            }
            ContainmentPolicy::Fail => {
                return Err(anyhow!(
                    "mutate containment violation: check '{}' modified files outside lane scope: {}",
                    check.id,
                    outside.join(", ")
                ));
            }
        }
    }

    let to_stage: Vec<String> = after_unstaged
        .iter()
        .filter(|p| lane_set.contains(p.as_str()))
        .cloned()
        .collect();
    if !to_stage.is_empty() {
        git_add(repo_root, &to_stage)?;
    }

    Ok(())
}

fn parse_containment(s: Option<&str>) -> ContainmentPolicy {
    match s.unwrap_or("off") {
        "fail" => ContainmentPolicy::Fail,
        "warn" => ContainmentPolicy::Warn,
        _ => ContainmentPolicy::Off,
    }
}

fn parse_restage(s: Option<&str>) -> RestagePolicy {
    match s.unwrap_or("off") {
        "auto" => RestagePolicy::Auto,
        _ => RestagePolicy::Off,
    }
}

fn fileset_scope_for_lane(scope_base: Option<&str>, check_id: &str) -> Result<FilesetScope> {
    match scope_base {
        Some("staged") => Ok(FilesetScope::Staged),
        Some("uncommitted") => Ok(FilesetScope::Uncommitted),
        Some("committed_not_pushed") => Ok(FilesetScope::CommittedNotPushed),
        Some("head") => Ok(FilesetScope::Head),
        Some(other) => Err(anyhow!(
            "check '{check_id}' has input_mode='paths' but lane scope.base='{other}' is not supported"
        )),
        None => Err(anyhow!(
            "check '{check_id}' has input_mode='paths' but lane scope.base is missing"
        )),
    }
}

fn runner_config_from_v3(defaults: &DefaultsV3) -> RunnerConfig {
    let mut config = RunnerConfig::default();

    if let Some(seconds) = defaults.silence_warning_seconds {
        config.silence_warning_seconds = seconds;
    }

    if let Some(simple) = defaults.simple_output {
        config.simple_output = simple;
    }

    if let Some(show_parallel) = defaults.show_parallel_output {
        config.show_parallel_output = show_parallel;
    }

    if let Some(parallel) = defaults.parallel {
        config.parallel = parallel;
    }

    config
}

fn resolve_check_ref<'a>(check_ref: &'a CheckRefV3, config: &'a ConfigV3) -> Result<&'a CheckV3> {
    match check_ref {
        CheckRefV3::Ref(name) => config
            .check
            .get(name)
            .ok_or_else(|| anyhow!("unknown check '{name}'")),
        CheckRefV3::Inline(check) => Ok(check),
    }
}

fn parse_workflow_scope(scope: Option<&str>) -> FilesetScope {
    match scope {
        Some("staged") => FilesetScope::Staged,
        Some("uncommitted") => FilesetScope::Uncommitted,
        Some("committed_not_pushed") | Some("committed-not-pushed") => {
            FilesetScope::CommittedNotPushed
        }
        Some("head") | Some("all") => FilesetScope::Head,
        None => FilesetScope::Uncommitted,
        Some(_) => FilesetScope::Uncommitted,
    }
}

#[allow(clippy::too_many_arguments)]
fn run_check_plans(
    lane: &str,
    plans: Vec<CheckPlan>,
    lane_parallel: bool,
    scope_base: Option<String>,
    runner_config: RunnerConfig,
    repo_root: PathBuf,
    format: OutputFormat,
    verbose: bool,
    color: ColorMode,
) -> Result<()> {
    let lane_start = Instant::now();

    let use_color = colors_enabled(color);

    // Smart partitioning: separate parallel-safe checks from sequential mutate+restage checks.
    // This allows us to run observe checks in parallel while mutate checks run sequentially.
    let is_staged_scope = scope_base.as_deref() == Some("staged");

    // Partition into (parallel_indices, sequential_indices) while preserving original indices
    let (parallel_indices, sequential_indices): (Vec<usize>, Vec<usize>) =
        if lane_parallel && is_staged_scope {
            let mut par = Vec::new();
            let mut seq = Vec::new();
            for (idx, plan) in plans.iter().enumerate() {
                if plan.category == CheckCategory::Mutate && plan.restage == RestagePolicy::Auto {
                    seq.push(idx);
                } else {
                    par.push(idx);
                }
            }
            (par, seq)
        } else if lane_parallel {
            // No staged scope, so all checks can run in parallel
            ((0..plans.len()).collect(), Vec::new())
        } else {
            // Not parallel, all sequential
            (Vec::new(), (0..plans.len()).collect())
        };

    let has_parallel = !parallel_indices.is_empty();
    let has_sequential = !sequential_indices.is_empty();

    // Info message if we're doing smart partitioning (both groups non-empty)
    if has_parallel && has_sequential {
        eprintln!(
            "info: lane '{lane}' using smart scheduling: {} parallel + {} sequential mutate checks",
            parallel_indices.len(),
            sequential_indices.len()
        );
    }

    // Runner: execute parallel group first, then sequential group.
    let mut results: Vec<Option<Result<CheckResult>>> = (0..plans.len()).map(|_| None).collect();

    // Detect if we're running on a TTY for spinner support.
    let is_tty = Term::stdout().is_term();

    // Detect terminal config for adaptive rendering
    let term_config = if is_tty {
        TerminalConfig::detect()
    } else {
        TerminalConfig::for_ci()
    };

    // Register resize handler (no-op on non-Unix)
    let _ = TerminalConfig::register_resize_handler();

    // For TTY mode: create progress groups for ALL checks upfront
    // This enables a unified updater thread for both parallel and sequential execution
    let stream_output = verbose || format == OutputFormat::Grouped;
    let mp = if is_tty && !stream_output {
        Some(MultiProgress::new())
    } else {
        None
    };

    // Create progress groups for all checks (TTY mode only)
    let progress_groups: Vec<Option<CheckProgressGroup>> = if let Some(ref mp) = mp {
        plans
            .iter()
            .map(|p| {
                Some(CheckProgressGroup::new_with_config(
                    mp,
                    &p.label,
                    term_config.label_padding,
                    term_config.show_context_lines,
                ))
            })
            .collect()
    } else {
        plans.iter().map(|_| None).collect()
    };

    // Start unified updater thread for all progress groups
    let done_flag = Arc::new(AtomicBool::new(false));
    let updater_handle = if mp.is_some() {
        let done_flag_clone = done_flag.clone();
        let groups_for_updater: Vec<_> = progress_groups.iter().flatten().cloned().collect();
        let labels_for_updater: Vec<_> = plans.iter().map(|p| p.label.clone()).collect();
        let silence_warning_seconds = runner_config.silence_warning_seconds;
        let label_padding_for_updater = term_config.label_padding;
        let truncation_limit_for_updater = term_config.truncation_limit;
        let use_compact_indicator = term_config.use_compact_indicator;
        let use_color_for_updater = use_color;

        // Debug file for updater thread
        #[cfg(debug_assertions)]
        let debug_file = if std::env::var("EXOHOOK_DEBUG_UPDATER").is_ok() {
            use std::fs::OpenOptions;
            Some(std::sync::Mutex::new(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("/tmp/exohook-updater-debug.log")
                    .ok(),
            ))
        } else {
            None
        };

        Some(thread::spawn(move || {
            let mut silence_warned: Vec<bool> = vec![false; groups_for_updater.len()];

            while !done_flag_clone.load(Ordering::Relaxed) {
                for (idx, group) in groups_for_updater.iter().enumerate() {
                    // Skip if already finished
                    if group.spinner.is_finished() {
                        continue;
                    }

                    let label = &labels_for_updater[idx];

                    // Check if this task has actually started
                    if !group.is_started() {
                        // Show "waiting..." for queued but not-yet-started tasks
                        // (no context update since no output yet)
                        continue;
                    }

                    // Debug: log buffer stats
                    #[cfg(debug_assertions)]
                    #[allow(clippy::collapsible_if)]
                    if let Some(ref df) = debug_file {
                        if let Some(f) = df.lock().unwrap().as_mut() {
                            use std::io::Write;
                            let stats = group.output_buffer.debug_stats();
                            let snapshot = group.output_buffer.snapshot();
                            let arc_id = group.output_buffer.arc_id();
                            let _ = writeln!(
                                f,
                                "[UPDATER] {}: arc={:x}, bytes={}, lines={}, current_len={}, snapshot={:?}",
                                label, arc_id, stats.0, stats.1, stats.2, snapshot
                            );
                        }
                    }

                    // Get per-check elapsed time
                    let check_elapsed = group.elapsed().unwrap_or(Duration::ZERO);

                    let silence = group.output_buffer.silence_duration();
                    let show_silence_warning = silence_warning_seconds > 0
                        && silence.as_secs() >= silence_warning_seconds
                        && !silence_warned[idx];

                    if show_silence_warning {
                        silence_warned[idx] = true;
                        group.spinner.set_message(format!(
                            "⚠ {} has been silent for {}s — still working, please be patient",
                            label, silence_warning_seconds
                        ));
                    } else if use_compact_indicator {
                        group.spinner.set_message(format!(
                            "{} {}",
                            label,
                            compact_progress_indicator(check_elapsed.as_secs())
                        ));
                    } else {
                        group.spinner.set_message(format!(
                            "{:<width$} running... ({:.1}s, please wait)",
                            label,
                            check_elapsed.as_secs_f64(),
                            width = label_padding_for_updater
                        ));
                    }

                    group.update_context(use_color_for_updater, truncation_limit_for_updater);
                }
                thread::sleep(Duration::from_millis(50));
            }
        }))
    } else {
        None
    };

    // =====================================================================
    // PHASE 1: Run parallel-safe checks concurrently
    // =====================================================================
    if has_parallel {
        let term_width = term_config.width;
        let force_pipes = runner_config.simple_output;

        if mp.is_some() {
            // TTY mode: spawn threads with progress group buffers
            let mut handles = Vec::with_capacity(parallel_indices.len());
            for &original_idx in &parallel_indices {
                let plan = plans[original_idx].clone();
                let buffer = progress_groups[original_idx]
                    .as_ref()
                    .map(|g| g.output_buffer.clone());

                // Debug: log buffer identity at spawn time
                #[cfg(debug_assertions)]
                if std::env::var("EXOHOOK_DEBUG_SPAWN").is_ok() {
                    if let Some(ref b) = buffer {
                        eprintln!("[SPAWN] {}: passed_arc={:x}", plan.label, b.arc_id());
                    }
                    if let Some(ref g) = progress_groups[original_idx] {
                        eprintln!(
                            "[SPAWN] {}: group_arc={:x}",
                            plan.label,
                            g.output_buffer.arc_id()
                        );
                    }
                }

                // Mark this check as started before spawning the thread
                if let Some(group) = &progress_groups[original_idx] {
                    group.mark_started();
                }
                handles.push((
                    original_idx,
                    thread::spawn(move || {
                        let (program, args) = plan
                            .parts
                            .split_first()
                            .ok_or_else(|| anyhow!("check '{}' argv must not be empty", plan.id))?;
                        run_command_streaming(
                            &plan.workdir,
                            program,
                            args,
                            false,
                            buffer,
                            force_pipes,
                            term_width,
                        )
                    }),
                ));
            }

            for (original_idx, h) in handles {
                let res = match h.join() {
                    Ok(v) => v,
                    Err(_) => Err(anyhow!("check thread panicked")),
                };

                // Update progress group based on result
                if let Some(group) = &progress_groups[original_idx] {
                    let label = &plans[original_idx].label;
                    match &res {
                        Ok(ok) if ok.status.success() => {
                            group.finish_success(
                                label,
                                ok.duration,
                                term_config.label_padding,
                                use_color,
                            );
                        }
                        Ok(ok) => {
                            group.finish_failure(
                                label,
                                ok.duration,
                                term_config.label_padding,
                                use_color,
                            );
                        }
                        Err(_) => {
                            group.finish_failure(
                                label,
                                Duration::ZERO,
                                term_config.label_padding,
                                use_color,
                            );
                        }
                    }
                }

                results[original_idx] = Some(res);
            }
        } else {
            // Non-TTY: simple parallel execution
            let mut handles = Vec::with_capacity(parallel_indices.len());
            for &original_idx in &parallel_indices {
                let plan = plans[original_idx].clone();
                handles.push((
                    original_idx,
                    thread::spawn(move || {
                        let (program, args) = plan
                            .parts
                            .split_first()
                            .ok_or_else(|| anyhow!("check '{}' argv must not be empty", plan.id))?;
                        run_command_capture(&plan.workdir, program, args)
                    }),
                ));
            }

            for (original_idx, h) in handles {
                let res = match h.join() {
                    Ok(v) => v,
                    Err(_) => Err(anyhow!("check thread panicked")),
                };
                results[original_idx] = Some(res);
            }
        }
    }

    // =====================================================================
    // PHASE 2: Run sequential mutate+restage checks (if any)
    // =====================================================================
    for &original_idx in &sequential_indices {
        let plan = plans[original_idx].clone();
        let before_unstaged = if plan.restage == RestagePolicy::Auto {
            git_lines(&repo_root, &["diff", "--name-only"]).unwrap_or_default()
        } else {
            Vec::new()
        };

        let (program, args) = plan
            .parts
            .split_first()
            .ok_or_else(|| anyhow!("check '{}' argv must not be empty", plan.id))?;

        let res = if let Some(group) = &progress_groups[original_idx] {
            // TTY mode: use the pre-created progress group
            // Mark this check as started now
            group.mark_started();
            let cmd_res = run_command_streaming(
                &plan.workdir,
                program,
                args,
                false,
                Some(group.output_buffer.clone()),
                runner_config.simple_output,
                term_config.width,
            );

            // Update progress group based on result
            match &cmd_res {
                Ok(ok) if ok.status.success() => {
                    group.finish_success(
                        &plan.label,
                        ok.duration,
                        term_config.label_padding,
                        use_color,
                    );
                }
                Ok(ok) => {
                    group.finish_failure(
                        &plan.label,
                        ok.duration,
                        term_config.label_padding,
                        use_color,
                    );
                }
                Err(_) => {
                    group.finish_failure(
                        &plan.label,
                        Duration::ZERO,
                        term_config.label_padding,
                        use_color,
                    );
                }
            }

            cmd_res
        } else {
            // Non-TTY or verbose/grouped mode: stream output directly
            if stream_output {
                println!("==> {} ({})", plan.id, plan.label);
            }
            run_command_streaming(
                &plan.workdir,
                program,
                args,
                stream_output,
                None,
                runner_config.simple_output,
                term_config.width,
            )
        };

        if let Ok(ref ok) = res
            && ok.status.success()
            && plan.restage == RestagePolicy::Auto
        {
            let after_unstaged =
                git_lines(&repo_root, &["diff", "--name-only"]).unwrap_or_default();
            if let Err(e) = apply_restage_if_needed(
                &repo_root,
                scope_base.as_deref(),
                &plan,
                &before_unstaged,
                &after_unstaged,
            ) {
                results[original_idx] = Some(Err(e));
                break;
            }
        }

        let should_stop = match &res {
            Ok(ok) => !ok.status.success(),
            Err(_) => true,
        };
        results[original_idx] = Some(res);

        if should_stop {
            break;
        }
    }

    // Stop updater thread
    done_flag.store(true, Ordering::Relaxed);
    if let Some(h) = updater_handle {
        h.join().ok();
    }

    // Collect results with their original plan indices, filtering out None (unfilled slots from fail-fast)
    let ran_results: Vec<(usize, Result<CheckResult>)> = results
        .into_iter()
        .enumerate()
        .filter_map(|(idx, opt)| opt.map(|r| (idx, r)))
        .collect();

    // Print outputs in stable config order for the checks that actually ran.
    // Note: For sequential execution with streaming, output was already printed inline,
    // so we only need to print summaries and failure details here.
    let ran = ran_results.len();
    let mut failed = Vec::new();
    let mut first_err: Option<anyhow::Error> = None;

    // Whether output was already streamed (sequential with verbose/grouped)
    let already_streamed = has_sequential && (verbose || format == OutputFormat::Grouped);

    for (idx, res) in &ran_results {
        let plan = &plans[*idx];
        match (format, res) {
            (OutputFormat::Grouped, Ok(ok)) => {
                // For parallel execution, print all grouped output now
                // For sequential, output was already streamed
                if has_parallel {
                    print_grouped_output(plan, ok);
                }
                if !ok.status.success() {
                    failed.push(plan.id.clone());
                }
            }
            (OutputFormat::Grouped, Err(e)) => {
                if !already_streamed {
                    eprintln!("==> {} ({})", plan.id, plan.label);
                }
                eprintln!("error: {e}");
                if first_err.is_none() {
                    first_err = Some(anyhow!(e.to_string()));
                }
                failed.push(plan.id.clone());
            }

            (OutputFormat::Compact, Ok(ok)) => {
                let ok_status = ok.status.success();

                // For parallel with spinners, status was shown via spinner
                // For sequential without streaming, show status line
                if !already_streamed && !has_parallel {
                    if verbose || !ok_status {
                        print_compact_status_line(
                            plan,
                            ok_status,
                            ok.duration,
                            use_color,
                            term_config.label_padding,
                        );
                    }
                } else if has_parallel && !is_tty {
                    // Non-TTY parallel: print status lines
                    if verbose || !ok_status {
                        print_compact_status_line(
                            plan,
                            ok_status,
                            ok.duration,
                            use_color,
                            term_config.label_padding,
                        );
                    }
                }

                if !ok_status {
                    // Always print failure details if not already streamed
                    if !already_streamed {
                        print_compact_failure_details(plan, ok);
                    }
                    failed.push(plan.id.clone());
                }
            }
            (OutputFormat::Compact, Err(e)) => {
                if !(already_streamed || (has_parallel && is_tty)) {
                    print_compact_status_line(
                        plan,
                        false,
                        Duration::from_millis(0),
                        use_color,
                        term_config.label_padding,
                    );
                }
                println!("error: {e}");
                if first_err.is_none() {
                    first_err = Some(anyhow!(e.to_string()));
                }
                failed.push(plan.id.clone());
            }

            // JSONL format is handled separately - this path should not be reached
            // when format is Jsonl because JSONL mode uses a different execution path
            (OutputFormat::Jsonl, _) => {
                unreachable!("JSONL format should use dedicated streaming path");
            }
        }
    }

    if format == OutputFormat::Compact {
        let elapsed = lane_start.elapsed();
        let passed = ran - failed.len();
        println!(
            "{}",
            format_lane_summary(lane, passed, plans.len(), elapsed, use_color)
        );
    }

    if !failed.is_empty() {
        if !lane_parallel {
            if let Some(e) = first_err {
                return Err(e);
            }
            return Err(anyhow!("check '{}' failed", failed[0]));
        }
        return Err(anyhow!(
            "lane '{lane}' failed checks: {}",
            failed.join(", ")
        ));
    }

    Ok(())
}

pub(crate) fn validate_v3_hook(
    path: &Path,
    hook_name: &str,
    context: ExecutionContext,
    dry_run: bool,
    format: OutputFormat,
    verbose: bool,
    color: ColorMode,
    category: Option<CheckCategory>,
) -> Result<()> {
    let use_color = colors_enabled(color);

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let config = ConfigV3::parse(&content)?;
    config.validate()?;

    let workflow_ref = match context.hook_type {
        HookType::PreCommit => config.hooks.pre_commit.as_deref(),
        HookType::PrePush => config.hooks.pre_push.as_deref(),
        HookType::CommitMsg => config.hooks.commit_msg.as_deref(),
        HookType::PreMergeCommit => config.hooks.pre_merge_commit.as_deref(),
        HookType::Manual => None,
    };

    let workflow_name =
        workflow_ref.ok_or_else(|| anyhow!("no workflow configured for hook '{}'", hook_name))?;

    let workflow = config.workflow.get(workflow_name).ok_or_else(|| {
        anyhow!(
            "hooks.{} references unknown workflow '{}'",
            hook_name,
            workflow_name
        )
    })?;

    let checks = &workflow.checks;

    let scope = context.hook_type.inferred_scope();
    let scope_base = if scope == FilesetScope::Staged {
        Some("staged".to_string())
    } else {
        None
    };

    let repo_root = git_repo_root().unwrap_or_else(|_| std::env::current_dir().unwrap());
    let runner_config = runner_config_from_v3(&config.defaults);

    let mut fileset_cache: Option<Vec<String>> = None;
    let mut plans: Vec<CheckPlan> = Vec::new();
    let mut tool_failures: Vec<String> = Vec::new();
    let mut selected_check_count = 0;

    for (idx, check_ref) in checks.iter().enumerate() {
        let check_id = match check_ref {
            CheckRefV3::Ref(name) => name.clone(),
            CheckRefV3::Inline(_) => format!("{workflow_name}-inline-{idx}"),
        };
        let check = resolve_check_ref(check_ref, &config)?;
        if category.is_some_and(|category| check.category != category) {
            continue;
        }
        selected_check_count += 1;

        let label = check.label.as_deref().unwrap_or(&check_id);

        let workdir = if let Some(cwd) = check.cwd.as_deref() {
            repo_root.join(cwd)
        } else {
            repo_root.clone()
        };

        // Handle tool-based checks
        if let Some(tool) = &check.tool {
            // Parse tool address (e.g., "exo.docs.links.check" -> ["docs", "links", "check"])
            let tool_address: Vec<String> = tool
                .strip_prefix("exo.")
                .unwrap_or(tool)
                .split('.')
                .map(|s| s.to_string())
                .collect();

            // Get files if filters are specified
            let files = if !check.filters.is_empty() {
                if fileset_cache.is_none() {
                    let computed = compute_fileset(&repo_root, scope).map_err(|e| anyhow!(e))?;
                    fileset_cache = Some(computed);
                }
                let base_files = fileset_cache.as_ref().unwrap();
                let filtered = filter_files(base_files, &check.filters)?;

                if filtered.is_empty() && check.effective_skip_if_empty() {
                    if format == OutputFormat::Jsonl {
                        plans.push(CheckPlan {
                            id: check_id.clone(),
                            label: label.to_string(),
                            parts: Vec::new(),
                            workdir: workdir.clone(),
                            category: check.category,
                            restage: RestagePolicy::auto_if(context.should_restage(check)),
                            containment: ContainmentPolicy::Off,
                            tool: Some(ToolPlan {
                                id: check_id.clone(),
                                label: label.to_string(),
                                tool_address: tool_address.clone(),
                                files: Some(filtered),
                            }),
                            skipped: true,
                            filters: check.filters.clone(),
                            matched_files: Some(vec![]),
                        });
                    } else if use_color {
                        println!("\x1b[2m  - {} skipped (no matches)\x1b[0m", label);
                    } else {
                        println!("  - {} skipped (no matches)", label);
                    }
                    continue;
                }
                Some(filtered)
            } else {
                None
            };

            if dry_run {
                println!("tool: {} -> {}", check_id, tool_address.join("."));
                continue;
            }

            let matched_files_snapshot = files.clone();
            let tool_plan = ToolPlan {
                id: check_id.clone(),
                label: label.to_string(),
                tool_address,
                files,
            };

            if format == OutputFormat::Jsonl {
                plans.push(CheckPlan {
                    id: check_id.clone(),
                    label: label.to_string(),
                    parts: Vec::new(),
                    workdir: workdir.clone(),
                    category: check.category,
                    restage: RestagePolicy::auto_if(context.should_restage(check)),
                    containment: ContainmentPolicy::Off,
                    tool: Some(tool_plan),
                    skipped: false,
                    filters: check.filters.clone(),
                    matched_files: matched_files_snapshot,
                });
                continue;
            }

            print!("  ⚙ {}", label);
            std::io::stdout().flush().ok();

            match invoke_tool(&tool_plan) {
                Ok(result) => {
                    if result.success {
                        println!("\r  ✓ {}  {:>6.1}s", label, result.duration.as_secs_f64());
                    } else {
                        println!("\r  ✗ {}  {:>6.1}s", label, result.duration.as_secs_f64());
                        if let Some(msg) = result.message {
                            eprintln!("    error: {}", msg);
                        }
                        tool_failures.push(check_id);
                    }
                }
                Err(e) => {
                    println!("\r  ✗ {}  error", label);
                    eprintln!("    error: {}", e);
                    tool_failures.push(check_id);
                }
            }

            continue;
        }

        let should_fix = context.should_fix(check);
        let command = if should_fix {
            check.fix_command.as_ref().or(check.command.as_ref())
        } else {
            check.command.as_ref()
        };

        let Some(command) = command else {
            continue;
        };

        let wants_files = !check.filters.is_empty() || command.contains("{{files}}");
        let files_for_check = if wants_files {
            if fileset_cache.is_none() {
                let computed = compute_fileset(&repo_root, scope).map_err(|e| anyhow!(e))?;
                fileset_cache = Some(computed);
            }

            let base_files = fileset_cache.as_ref().unwrap();
            let filtered = filter_files(base_files, &check.filters)?;

            if filtered.is_empty() && check.effective_skip_if_empty() {
                if format == OutputFormat::Jsonl {
                    plans.push(CheckPlan {
                        id: check_id.clone(),
                        label: label.to_string(),
                        parts: shell_command_parts(command.to_string()),
                        workdir: workdir.clone(),
                        category: check.category,
                        restage: RestagePolicy::auto_if(context.should_restage(check)),
                        containment: ContainmentPolicy::Off,
                        tool: None,
                        skipped: true,
                        filters: check.filters.clone(),
                        matched_files: Some(vec![]),
                    });
                } else if use_color {
                    println!("\x1b[2m  - {} skipped (no matches)\x1b[0m", label);
                } else {
                    println!("  - {} skipped (no matches)", label);
                }
                continue;
            }

            Some(filtered)
        } else {
            None
        };

        let command_str = if let Some(files) = files_for_check.as_ref() {
            let rebased = rebase_files_for_cwd(files, check.cwd.as_deref());
            substitute_files(command, &rebased)
        } else {
            command.to_string()
        };
        let parts = shell_command_parts(command_str);

        if dry_run {
            println!("{}", parts.join(" "));
            continue;
        }

        let label = label.to_string();

        let restage = if context.should_restage(check) {
            RestagePolicy::Auto
        } else {
            RestagePolicy::Off
        };

        plans.push(CheckPlan {
            id: check_id,
            label,
            parts,
            workdir,
            category: check.category,
            restage,
            containment: ContainmentPolicy::Off,
            tool: None,
            skipped: false,
            filters: check.filters.clone(),
            matched_files: files_for_check.clone(),
        });
    }

    report_empty_category_selection(format, category, workflow_name, selected_check_count);

    if dry_run {
        return Ok(());
    }

    // Run command-based checks
    let command_result = if format == OutputFormat::Jsonl {
        let emitter = Arc::new(JsonlEmitter::stdout());
        run_check_plans_jsonl(
            hook_name,
            plans,
            runner_config.parallel,
            scope_base,
            repo_root,
            emitter,
        )
    } else if plans.is_empty() {
        Ok(())
    } else {
        run_check_plans(
            hook_name,
            plans,
            runner_config.parallel,
            scope_base,
            runner_config,
            repo_root,
            format,
            verbose,
            color,
        )
    };

    // Report tool failures
    if !tool_failures.is_empty() && format != OutputFormat::Jsonl {
        return Err(anyhow!(
            "tool check(s) failed: {}",
            tool_failures.join(", ")
        ));
    }

    command_result
}

pub fn validate_v3_workflow(
    path: &Path,
    workflow_name: &str,
    dry_run: bool,
    format: OutputFormat,
    verbose: bool,
    color: ColorMode,
    category: Option<CheckCategory>,
) -> Result<()> {
    let use_color = colors_enabled(color);

    let doc = read_hooks_doc(path)?;
    let config = ConfigV3::from_doc(&doc)?;
    config.validate()?;

    let workflow = config.workflow.get(workflow_name).ok_or_else(|| {
        let available: Vec<_> = config.workflow.keys().map(String::as_str).collect();
        if available.is_empty() {
            anyhow!(
                "workflow '{}' not found (no workflows defined)",
                workflow_name
            )
        } else {
            anyhow!(
                "workflow '{}' not found. Available: {}",
                workflow_name,
                available.join(", ")
            )
        }
    })?;

    let scope = parse_workflow_scope(workflow.scope.as_deref());

    let context = ExecutionContext {
        hook_type: HookType::Manual,
        is_interactive: true,
        force_fix: workflow.fix_policy.as_deref() == Some("always"),
        force_no_fix: workflow.fix_policy.as_deref() == Some("never"),
    };

    let repo_root = git_repo_root().unwrap_or_else(|_| std::env::current_dir().unwrap());
    let runner_config = runner_config_from_v3(&config.defaults);

    let mut fileset_cache: Option<Vec<String>> = None;
    let mut plans: Vec<CheckPlan> = Vec::new();
    let mut tool_failures: Vec<String> = Vec::new();
    let mut selected_check_count = 0;

    for (idx, check_ref) in workflow.checks.iter().enumerate() {
        let check_id = match check_ref {
            CheckRefV3::Ref(name) => name.clone(),
            CheckRefV3::Inline(_) => format!("{workflow_name}-inline-{idx}"),
        };
        let check = resolve_check_ref(check_ref, &config)?;
        if category.is_some_and(|category| check.category != category) {
            continue;
        }
        selected_check_count += 1;
        let label = check.label.as_deref().unwrap_or(&check_id);

        let workdir = if let Some(cwd) = check.cwd.as_deref() {
            repo_root.join(cwd)
        } else {
            repo_root.clone()
        };

        if let Some(tool) = &check.tool {
            let tool_address: Vec<String> = tool
                .strip_prefix("exo.")
                .unwrap_or(tool)
                .split('.')
                .map(|s| s.to_string())
                .collect();

            let files = if !check.filters.is_empty() {
                if fileset_cache.is_none() {
                    let computed = compute_fileset(&repo_root, scope).map_err(|e| anyhow!(e))?;
                    fileset_cache = Some(computed);
                }
                let base_files = fileset_cache.as_ref().unwrap();
                let filtered = filter_files(base_files, &check.filters)?;

                if filtered.is_empty() && check.effective_skip_if_empty() {
                    if format == OutputFormat::Jsonl {
                        plans.push(CheckPlan {
                            id: check_id.clone(),
                            label: label.to_string(),
                            parts: Vec::new(),
                            workdir: workdir.clone(),
                            category: check.category,
                            restage: RestagePolicy::auto_if(context.should_restage(check)),
                            containment: ContainmentPolicy::Off,
                            tool: Some(ToolPlan {
                                id: check_id.clone(),
                                label: label.to_string(),
                                tool_address: tool_address.clone(),
                                files: Some(filtered),
                            }),
                            skipped: true,
                            filters: check.filters.clone(),
                            matched_files: Some(vec![]),
                        });
                    } else if use_color {
                        println!("\x1b[2m  - {} skipped (no matches)\x1b[0m", label);
                    } else {
                        println!("  - {} skipped (no matches)", label);
                    }
                    continue;
                }
                Some(filtered)
            } else {
                None
            };

            if dry_run {
                println!("tool: {} -> {}", check_id, tool_address.join("."));
                continue;
            }

            let matched_files_snapshot = files.clone();
            let tool_plan = ToolPlan {
                id: check_id.clone(),
                label: label.to_string(),
                tool_address,
                files,
            };

            if format == OutputFormat::Jsonl {
                plans.push(CheckPlan {
                    id: check_id.clone(),
                    label: label.to_string(),
                    parts: Vec::new(),
                    workdir: workdir.clone(),
                    category: check.category,
                    restage: RestagePolicy::auto_if(context.should_restage(check)),
                    containment: ContainmentPolicy::Off,
                    tool: Some(tool_plan),
                    skipped: false,
                    filters: check.filters.clone(),
                    matched_files: matched_files_snapshot,
                });
                continue;
            }

            print!("  ⚙ {}", label);
            std::io::stdout().flush().ok();

            match invoke_tool(&tool_plan) {
                Ok(result) => {
                    if result.success {
                        println!("\r  ✓ {}  {:>6.1}s", label, result.duration.as_secs_f64());
                    } else {
                        println!("\r  ✗ {}  {:>6.1}s", label, result.duration.as_secs_f64());
                        if let Some(msg) = result.message {
                            eprintln!("    error: {}", msg);
                        }
                        tool_failures.push(check_id);
                    }
                }
                Err(e) => {
                    println!("\r  ✗ {}  error", label);
                    eprintln!("    error: {}", e);
                    tool_failures.push(check_id);
                }
            }

            continue;
        }

        let should_fix = context.should_fix(check);
        let command = if should_fix {
            check.fix_command.as_ref().or(check.command.as_ref())
        } else {
            check.command.as_ref()
        };

        let Some(command) = command else {
            continue;
        };

        let wants_files = !check.filters.is_empty() || command.contains("{{files}}");
        let files_for_check = if wants_files {
            if fileset_cache.is_none() {
                let computed = compute_fileset(&repo_root, scope).map_err(|e| anyhow!(e))?;
                fileset_cache = Some(computed);
            }
            let base_files = fileset_cache.as_ref().unwrap();
            let filtered = filter_files(base_files, &check.filters)?;

            if filtered.is_empty() && check.effective_skip_if_empty() {
                if format == OutputFormat::Jsonl {
                    plans.push(CheckPlan {
                        id: check_id.clone(),
                        label: label.to_string(),
                        parts: shell_command_parts(command.to_string()),
                        workdir: workdir.clone(),
                        category: check.category,
                        restage: RestagePolicy::auto_if(context.should_restage(check)),
                        containment: ContainmentPolicy::Off,
                        tool: None,
                        skipped: true,
                        filters: check.filters.clone(),
                        matched_files: Some(vec![]),
                    });
                } else if use_color {
                    println!("\x1b[2m  - {} skipped (no matches)\x1b[0m", label);
                } else {
                    println!("  - {} skipped (no matches)", label);
                }
                continue;
            }
            Some(filtered)
        } else {
            None
        };

        let command_str = if let Some(files) = files_for_check.as_ref() {
            let rebased = rebase_files_for_cwd(files, check.cwd.as_deref());
            substitute_files(command, &rebased)
        } else {
            command.to_string()
        };
        let parts = shell_command_parts(command_str);

        if dry_run {
            println!("{}", parts.join(" "));
            continue;
        }

        let label = label.to_string();

        let restage = RestagePolicy::auto_if(context.should_restage(check));

        plans.push(CheckPlan {
            id: check_id,
            label,
            parts,
            workdir,
            category: check.category,
            restage,
            containment: ContainmentPolicy::Off,
            tool: None,
            skipped: false,
            filters: check.filters.clone(),
            matched_files: files_for_check.clone(),
        });
    }

    let empty_category_selection =
        report_empty_category_selection(format, category, workflow_name, selected_check_count);

    if dry_run {
        return Ok(());
    }

    let command_result = if format == OutputFormat::Jsonl {
        let emitter = Arc::new(JsonlEmitter::stdout());
        run_check_plans_jsonl(
            workflow_name,
            plans,
            workflow.parallel,
            None,
            repo_root,
            emitter,
        )
    } else if plans.is_empty() {
        if !empty_category_selection {
            println!("No checks to run in workflow '{}'.", workflow_name);
        }
        Ok(())
    } else {
        run_check_plans(
            workflow_name,
            plans,
            workflow.parallel,
            None,
            runner_config,
            repo_root,
            format,
            verbose,
            color,
        )
    };

    if !tool_failures.is_empty() && format != OutputFormat::Jsonl {
        return Err(anyhow!(
            "tool check(s) failed: {}",
            tool_failures.join(", ")
        ));
    }

    command_result
}

pub(crate) fn validate_from_config(
    path: &Path,
    lane: &str,
    dry_run: bool,
    format: OutputFormat,
    verbose: bool,
    color: ColorMode,
) -> Result<()> {
    let use_color = colors_enabled(color);

    let doc = read_hooks_doc(path)?;
    validate_hooks_doc(&doc)?;

    // Parse runner configuration from [defaults] section
    let runner_config = parse_runner_config(&doc);

    let repo_root = git_repo_root().unwrap_or_else(|_| std::env::current_dir().unwrap());

    let lane_table = match get_lane_table(&doc, lane) {
        Some(t) => t,
        None => {
            // Show the lane listing with the invalid lane name
            return show_lane_listing(color, Some(lane));
        }
    };

    let lane_parallel = lane_table
        .get("parallel")
        .and_then(Item::as_bool)
        .unwrap_or(false);

    let scope_base = lane_scope_base(lane_table);

    let lane_checks = lane_table
        .get("checks")
        .and_then(Item::as_array)
        .ok_or_else(|| anyhow!("lane '{lane}' is missing checks array"))?;

    let mut fileset_cache: Option<Vec<String>> = None;

    let mut plans: Vec<CheckPlan> = Vec::new();

    for check_id_item in lane_checks.iter() {
        let check_id = check_id_item
            .as_str()
            .ok_or_else(|| anyhow!("lane '{lane}' contains non-string check id"))?;

        let check_table = get_check_table(&doc, check_id)
            .ok_or_else(|| anyhow!("lane '{lane}' references unknown check '{check_id}'"))?;

        let input_mode = check_table
            .get("input_mode")
            .and_then(Item::as_str)
            .unwrap_or("none");

        if input_mode != "none" && input_mode != "paths" {
            return Err(anyhow!(
                "check '{check_id}' uses unsupported input_mode='{input_mode}'"
            ));
        }

        let label = check_table
            .get("label")
            .and_then(Item::as_str)
            .unwrap_or(check_id);

        let autofix = check_table
            .get("autofix")
            .and_then(Item::as_bool)
            .unwrap_or(false);
        let category = if autofix {
            CheckCategory::Mutate
        } else {
            CheckCategory::Observe
        };

        let (restage, containment) = if let Some(ov) = lane_override_table(&doc, lane, check_id) {
            (
                parse_restage(ov.get("restage").and_then(Item::as_str)),
                parse_containment(ov.get("restage_containment").and_then(Item::as_str)),
            )
        } else {
            (RestagePolicy::Off, ContainmentPolicy::Off)
        };

        let files_for_check = if input_mode == "paths" {
            if fileset_cache.is_none() {
                let scope = fileset_scope_for_lane(scope_base.as_deref(), check_id)?;
                let computed = compute_fileset(&repo_root, scope).map_err(|e| anyhow!(e))?;
                fileset_cache = Some(computed);
            }

            let files = fileset_cache.as_ref().unwrap();
            if files.is_empty() {
                if format == OutputFormat::Jsonl {
                    plans.push(CheckPlan {
                        id: check_id.to_string(),
                        label: label.to_string(),
                        parts: Vec::new(),
                        workdir: repo_root.clone(),
                        category,
                        restage,
                        containment,
                        tool: None,
                        skipped: true,
                        filters: vec![],
                        matched_files: Some(vec![]),
                    });
                } else if use_color {
                    println!("\x1b[2m  - {} skipped (no matches)\x1b[0m", label);
                } else {
                    println!("  - {} skipped (no matches)", label);
                }
                continue;
            }
            Some(files.as_slice())
        } else {
            None
        };

        let parts =
            resolve_check_command_parts(check_id, check_table, input_mode, files_for_check)?;

        let label = label.to_string();

        if dry_run {
            println!("{}", parts.join(" "));
            continue;
        }

        plans.push(CheckPlan {
            id: check_id.to_string(),
            label,
            parts,
            workdir: repo_root.clone(),
            category,
            restage,
            containment,
            tool: None,
            skipped: false,
            filters: vec![],
            matched_files: files_for_check.map(|f| f.to_vec()),
        });
    }

    if dry_run {
        return Ok(());
    }

    if format == OutputFormat::Jsonl {
        let emitter = Arc::new(JsonlEmitter::stdout());
        run_check_plans_jsonl(lane, plans, lane_parallel, scope_base, repo_root, emitter)
    } else {
        run_check_plans(
            lane,
            plans,
            lane_parallel,
            scope_base,
            runner_config,
            repo_root,
            format,
            verbose,
            color,
        )
    }
}

pub(crate) fn lane_scope_base(lane: &Table) -> Option<String> {
    let scope = lane.get("scope")?;

    if let Some(t) = scope.as_table() {
        return t.get("base").and_then(Item::as_str).map(|s| s.to_string());
    }

    if let Some(t) = scope.as_inline_table() {
        return t
            .get("base")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }

    None
}
