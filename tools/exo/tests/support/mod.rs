#![allow(clippy::disallowed_methods)] // integration harness uses real process APIs

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

pub mod case;
pub mod template;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEvent {
    pub stream: Stream,
    pub line: String,
}

#[derive(Debug)]
pub struct ExoRun {
    pub status: ExitStatus,
    pub interleaved_lines: Vec<LineEvent>,
    pub stdout: String,
    #[allow(dead_code)]
    pub stderr: String,
}

pub fn exo_bin() -> PathBuf {
    assert_cmd::cargo::cargo_bin!("exo").to_path_buf()
}

enum Msg {
    Line {
        at: Instant,
        seq: u64,
        stream: Stream,
        line: String,
    },
    Done {
        stream: Stream,
        buffer: String,
    },
}

fn spawn_reader<R: Read + Send + 'static>(
    stream: Stream,
    reader: R,
    tx: mpsc::Sender<Msg>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut seq: u64 = 0;
        let mut buffer = String::new();
        let mut br = BufReader::new(reader);
        let mut line = String::new();

        loop {
            line.clear();
            match br.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    // Keep a faithful-ish buffer (including newlines).
                    buffer.push_str(&line);

                    let trimmed = line.strip_suffix('\n').unwrap_or(&line);
                    let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);

                    seq += 1;
                    let _ = tx.send(Msg::Line {
                        at: Instant::now(),
                        seq,
                        stream,
                        line: trimmed.to_string(),
                    });
                }
            }
        }

        let _ = tx.send(Msg::Done { stream, buffer });
    })
}

pub fn run_exo_interleaved(cwd: &Path, args: &[&str]) -> ExoRun {
    // These integration tests assert on stderr output. Daemon mode prints its own
    // logs (startup/shutdown), which breaks the expected interleaving.
    //
    // Force direct execution unless the test explicitly asks otherwise.
    let mut full_args: Vec<&str> = Vec::with_capacity(args.len() + 1);
    if !args.iter().any(|a| *a == "--direct") {
        full_args.push("--direct");
    }
    full_args.extend_from_slice(args);

    // NOTE: deliberately no HOME/XDG isolation here. dispatch_parity compares
    // this spawned CLI against in-process handler calls (which read the real
    // process env), so both sides must resolve the same project policy.
    // Tests using this harness run pure reads or fixture-scoped writes.
    let mut cmd = Command::new(exo_bin());
    cmd.current_dir(cwd)
        .args(&full_args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn exo");

    let stdout = child.stdout.take().expect("stdout");
    let stderr = child.stderr.take().expect("stderr");

    let (tx, rx) = mpsc::channel::<Msg>();

    let t1 = spawn_reader(Stream::Stdout, stdout, tx.clone());
    let t2 = spawn_reader(Stream::Stderr, stderr, tx);

    let status = child.wait().expect("wait exo");

    let mut stdout_buf: Option<String> = None;
    let mut stderr_buf: Option<String> = None;
    let mut events: Vec<(Instant, u64, LineEvent)> = Vec::new();

    while stdout_buf.is_none() || stderr_buf.is_none() {
        match rx.recv().expect("recv") {
            Msg::Line {
                at,
                seq,
                stream,
                line,
            } => {
                events.push((at, seq, LineEvent { stream, line }));
            }
            Msg::Done { stream, buffer } => match stream {
                Stream::Stdout => stdout_buf = Some(buffer),
                Stream::Stderr => stderr_buf = Some(buffer),
            },
        }
    }

    let _ = t1.join();
    let _ = t2.join();

    events.sort_by(|(a_at, a_seq, _), (b_at, b_seq, _)| a_at.cmp(b_at).then(a_seq.cmp(b_seq)));

    ExoRun {
        status,
        interleaved_lines: events.into_iter().map(|(_, _, e)| e).collect(),
        stdout: stdout_buf.unwrap_or_default(),
        stderr: stderr_buf.unwrap_or_default(),
    }
}
