#![allow(missing_docs)]
#![allow(clippy::print_stdout, clippy::print_stderr)]

#[cfg(windows)]
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};

const USAGE: &str = "\
cargo test-sidecar-windows [--list] [--batch <name>] [--timeout-seconds <seconds>]

Runs Windows-focused sidecar integration test batches with process cleanup.
Use --batch more than once to run a subset.
";

const DEFAULT_TIMEOUT_SECS: u64 = 600;

const BATCHES: &[Batch] = &[
    Batch {
        name: "sidecar-discovery",
        filter: "sidecar_discover",
    },
    Batch {
        name: "sidecar-bootstrap-discovery",
        filter: "sidecar_bootstrap_discover",
    },
    Batch {
        name: "sidecar-repo-remote",
        filter: "sidecar_repo_remote",
    },
    Batch {
        name: "sidecar-repo-status",
        filter: "sidecar_repo_status",
    },
    Batch {
        name: "sidecar-repo-commit",
        filter: "sidecar_repo_commit",
    },
    Batch {
        name: "sidecar-repo-sync",
        filter: "sidecar_repo_sync",
    },
    Batch {
        name: "sidecar-repo-push",
        filter: "sidecar_repo_push",
    },
    Batch {
        name: "sidecar-daemon-status",
        filter: "sidecar_repo_status_reports_git_state",
    },
    Batch {
        name: "sidecar-daemon-commit",
        filter: "sidecar_repo_commit_without_direct_uses_daemon_and_keeps_work_repo_clean",
    },
];

#[derive(Clone, Copy)]
struct Batch {
    name: &'static str,
    filter: &'static str,
}

struct Args {
    list: bool,
    selected_batches: Vec<String>,
    timeout: Duration,
}

struct BatchOutcome {
    batch: &'static str,
    filter: &'static str,
    duration: Duration,
    status: BatchStatus,
    cleaned_temp_daemons: usize,
}

enum BatchStatus {
    Passed,
    Failed(String),
    TimedOut,
}

fn main() -> Result<()> {
    let args = parse_args()?;
    if args.list {
        for batch in BATCHES {
            println!("{}\t{}", batch.name, batch.filter);
        }
        return Ok(());
    }

    let root = exo_workspace::workspace_root()?;
    let batches = selected_batches(&args.selected_batches)?;
    let mut outcomes = Vec::new();
    let mut failed = false;

    for batch in batches {
        let outcome = run_batch(&root, batch, args.timeout)?;
        failed |= !matches!(outcome.status, BatchStatus::Passed);
        print_outcome(&outcome);
        outcomes.push(outcome);
    }

    println!("=== sidecar batch summary ===");
    for outcome in &outcomes {
        print_outcome(outcome);
    }

    if failed {
        bail!("one or more sidecar batches failed");
    }

    Ok(())
}

fn parse_args() -> Result<Args> {
    let mut list = false;
    let mut selected_batches = Vec::new();
    let mut timeout = Duration::from_secs(DEFAULT_TIMEOUT_SECS);
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "--list" => list = true,
            "--batch" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow!("--batch requires a batch name"))?;
                selected_batches.push(value);
            }
            "--timeout-seconds" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow!("--timeout-seconds requires a value"))?;
                let seconds = value
                    .parse::<u64>()
                    .with_context(|| format!("invalid timeout value `{value}`"))?;
                timeout = Duration::from_secs(seconds);
            }
            _ => bail!("unexpected argument `{arg}`\n\n{USAGE}"),
        }
    }

    Ok(Args {
        list,
        selected_batches,
        timeout,
    })
}

fn selected_batches(names: &[String]) -> Result<Vec<&'static Batch>> {
    if names.is_empty() {
        return Ok(BATCHES.iter().collect());
    }

    names
        .iter()
        .map(|name| {
            BATCHES
                .iter()
                .find(|batch| batch.name == name)
                .ok_or_else(|| anyhow!("unknown sidecar batch `{name}`"))
        })
        .collect()
}

fn run_batch(root: &Path, batch: &Batch, timeout: Duration) -> Result<BatchOutcome> {
    let mut command = Command::new("cargo");
    command
        .args([
            "test",
            "--locked",
            "-p",
            "exo",
            "--test",
            "sidecar",
            batch.filter,
            "--",
            "--test-threads=1",
            "--nocapture",
        ])
        .env("CARGO_INCREMENTAL", "0")
        .env("RUSTFLAGS", "-C debuginfo=0")
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let command_display = format!(
        "cargo test --locked -p exo --test sidecar {} -- --test-threads=1 --nocapture",
        batch.filter
    );
    println!("=== batch {} ===", batch.name);
    println!("command: {command_display}");

    let start = Instant::now();
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn {command_display}"))?;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break BatchStatus::from_exit_status(status);
        }
        if start.elapsed() >= timeout {
            terminate_process_tree(child.id(), root)?;
            let _ = child.wait();
            break BatchStatus::TimedOut;
        }
        thread::sleep(Duration::from_millis(250));
    };
    let cleaned_temp_daemons = cleanup_temp_sidecar_daemons(root)?;

    Ok(BatchOutcome {
        batch: batch.name,
        filter: batch.filter,
        duration: start.elapsed(),
        status,
        cleaned_temp_daemons,
    })
}

impl BatchStatus {
    fn from_exit_status(status: std::process::ExitStatus) -> Self {
        if status.success() {
            Self::Passed
        } else {
            Self::Failed(status.to_string())
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::Passed => "pass",
            Self::Failed(_) => "fail",
            Self::TimedOut => "timeout",
        }
    }
}

fn print_outcome(outcome: &BatchOutcome) {
    let detail = match &outcome.status {
        BatchStatus::Failed(status) => format!(" status={status}"),
        BatchStatus::Passed | BatchStatus::TimedOut => String::new(),
    };
    println!(
        "batch={} filter={} result={} duration={}s cleanup_temp_daemons={}{}",
        outcome.batch,
        outcome.filter,
        outcome.status.as_str(),
        outcome.duration.as_secs(),
        outcome.cleaned_temp_daemons,
        detail
    );
}

#[cfg(windows)]
fn terminate_process_tree(pid: u32, root: &Path) -> Result<()> {
    let script = format!(
        "$ErrorActionPreference = 'SilentlyContinue'; \
         function Stop-Tree([int]$Id) {{ \
           Get-CimInstance Win32_Process | Where-Object {{ $_.ParentProcessId -eq $Id }} | ForEach-Object {{ Stop-Tree $_.ProcessId }}; \
           if ($Id -ne $PID) {{ Stop-Process -Id $Id -Force -ErrorAction SilentlyContinue }} \
         }}; \
         Stop-Tree {pid}"
    );
    run_powershell(root, "terminate timed-out process tree", &script).map(|_| ())
}

#[cfg(not(windows))]
fn terminate_process_tree(pid: u32, _root: &Path) -> Result<()> {
    Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .with_context(|| format!("failed to terminate process {pid}"))?;
    Ok(())
}

#[cfg(windows)]
fn cleanup_temp_sidecar_daemons(root: &Path) -> Result<usize> {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$deadline = [DateTime]::UtcNow.AddSeconds(10)
$seen = @{}

do {
  $targets = @(Get-CimInstance Win32_Process | Where-Object {
    ($_.Name -eq 'exo.exe' -or $_.Name -eq 'exo-mcp.exe') -and
    $_.CommandLine -match 'exo-sidecar-'
  })

  foreach ($process in $targets) {
    $seen[[string]$process.ProcessId] = $true
    Stop-Process -Id $process.ProcessId -Force -ErrorAction SilentlyContinue
  }

  foreach ($process in $targets) {
    if (Get-Process -Id $process.ProcessId -ErrorAction SilentlyContinue) {
      Wait-Process -Id $process.ProcessId -Timeout 2 -ErrorAction SilentlyContinue
    }
  }

  $remaining = @(Get-CimInstance Win32_Process | Where-Object {
    ($_.Name -eq 'exo.exe' -or $_.Name -eq 'exo-mcp.exe') -and
    $_.CommandLine -match 'exo-sidecar-'
  })

  if ($remaining.Count -eq 0) { break }
  Start-Sleep -Milliseconds 250
} while ([DateTime]::UtcNow -lt $deadline)

Write-Output $seen.Count
if ($remaining.Count -gt 0) { exit 1 }
"#;
    let output = run_powershell(root, "clean up temp sidecar daemons", script)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .trim()
        .parse::<usize>()
        .with_context(|| format!("failed to parse cleanup count from `{stdout}`"))
}

#[cfg(not(windows))]
fn cleanup_temp_sidecar_daemons(_root: &Path) -> Result<usize> {
    Ok(0)
}

#[cfg(windows)]
fn run_powershell(root: &Path, operation: &str, script: &str) -> Result<std::process::Output> {
    let args: Vec<OsString> = vec!["-NoProfile".into(), "-Command".into(), script.into()];
    let output = Command::new("powershell.exe")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("failed to run PowerShell for {operation}"))?;
    if !output.status.success() {
        bail!(
            "PowerShell command for {operation} failed with status {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output)
}
