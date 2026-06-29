<!-- exo:113 ulid:01kg5kp2gnecv5kjzxxngthy72 -->

# RFC 113: Exohook Machine Channel Protocol


# RFC 0113: Exohook Machine Channel Protocol

## Summary

Define a machine-readable, streaming progress protocol for `exohook validate` using newline-delimited JSON (JSONL/NDJSON). This RFC specifies event types, JSON schemas, and streaming semantics for programmatic consumers (VS Code, CI, AI agents). It depends on the streaming execution infrastructure specified in RFC 0122.

## Motivation

PTY streaming solves the human UX problem, but tools need structured, real-time signals to:

- Render progress in UIs
- Provide live diagnostics in CI
- Enable agents to understand check status without parsing terminal output

A stable, documented protocol avoids ad-hoc parsing and unlocks integrations without coupling to terminal output formats.

## Detailed Design

### Terminology

- **Machine channel**: Structured JSONL output emitted to stdout.
- **Event**: One JSON object per line describing a discrete state change or output chunk.
- **Summary**: Final aggregate object emitted after all events.

### User Experience (UX)

Invocation (machine channel):

```bash
exohook validate gate --format=jsonl
```

Output stream (stdout):

```jsonl
{"type":"lane_started","lane":"gate","check_count":2,"timestamp":"2026-01-08T14:30:00.000Z"}
{"type":"check_started","check_id":"test","index":0,"command":"pnpm -r run test:unit","timestamp":"2026-01-08T14:30:00.001Z"}
{"type":"check_output","check_id":"test","stream":"stdout","data":"Running 47 tests...","timestamp":"2026-01-08T14:30:00.500Z"}
{"type":"check_completed","check_id":"test","status":"success","exit_code":0,"duration_ms":12300,"timestamp":"2026-01-08T14:30:12.301Z"}
{"type":"check_started","check_id":"rust-coverage","index":1,"command":"cargo llvm-cov","timestamp":"2026-01-08T14:30:12.302Z"}
{"type":"check_output","check_id":"rust-coverage","stream":"stdout","data":"Compiling exo v0.1.0","timestamp":"2026-01-08T14:30:13.000Z"}
{"type":"check_completed","check_id":"rust-coverage","status":"success","exit_code":0,"duration_ms":18500,"timestamp":"2026-01-08T14:30:30.802Z"}
{"type":"lane_completed","lane":"gate","status":"success","passed":2,"failed":0,"duration_ms":30802,"timestamp":"2026-01-08T14:30:30.803Z"}
```

### Architecture

Machine channel output uses the **pipe runner** exclusively to preserve separate stdout/stderr streams and avoid PTY merging. RFC 0122 provides the streaming execution infrastructure and output capture needed for this protocol.

### Why `--format=jsonl` (not `--format=json`)

We standardize on `--format=jsonl` across streaming commands:

1. **Simplicity**: One format flag to learn
2. **Streaming-ready**: JSONL is a superset of single-response JSON
3. **Consistency**: Single-result commands emit one line; streaming commands emit many
4. **Future-proof**: Adding streaming to any command doesn't require a flag change

### Event Types & Schemas

#### `lane_started`

```typescript
interface LaneStartedEvent {
  type: "lane_started";
  lane: string; // Lane ID (e.g., "gate", "ci", "coherence")
  check_count: number; // Total number of checks in this lane
  parallel: boolean; // Whether checks run in parallel
  timestamp: string; // ISO 8601 timestamp
}
```

#### `check_started`

```typescript
interface CheckStartedEvent {
  type: "check_started";
  check_id: string; // Check ID from hooks.toml
  index: number; // 0-based index within the lane
  command: string; // Shell command being executed
  working_dir?: string; // Working directory (if different from repo root)
  timestamp: string;
}
```

#### `check_output`

```typescript
interface CheckOutputEvent {
  type: "check_output";
  check_id: string;
  stream: "stdout" | "stderr";
  data: string; // Output chunk (may contain ANSI codes)
  timestamp: string;
}
```

#### `check_progress`

```typescript
interface CheckProgressEvent {
  type: "check_progress";
  check_id: string;
  elapsed_ms: number; // Time since check started
  output_bytes: number; // Total bytes of output so far
  timestamp: string;
}
```

#### `check_completed`

```typescript
interface CheckCompletedEvent {
  type: "check_completed";
  check_id: string;
  status: "success" | "failure" | "timeout" | "cancelled";
  exit_code: number | null; // null for timeout/cancelled
  duration_ms: number;
  output_bytes: number; // Total output size
  timestamp: string;
}
```

#### `lane_completed`

```typescript
interface LaneCompletedEvent {
  type: "lane_completed";
  lane: string;
  status: "success" | "failure";
  passed: number;
  failed: number;
  skipped: number; // Checks skipped due to prior failure (fail-fast)
  duration_ms: number; // Total wall-clock time
  timestamp: string;
}
```

#### `idle_warning`

```typescript
interface IdleWarningEvent {
  type: "idle_warning";
  check_id: string;
  silent_seconds: number; // How long since last output
  message: string; // Human-readable warning
  timestamp: string;
}
```

### Streaming Semantics

- **Line-delimited**: Each event is one JSON object per line.
- **Ordering**: Events are emitted in real time; parallel checks may interleave.
- **Output chunking**: Large outputs (>64KB per event) are split into multiple `check_output` events.
- **ANSI preservation**: `check_output.data` preserves ANSI escape sequences.
- **Partial lines**: Output chunks may contain partial lines; consumers must not assume line boundaries.

### Summary Envelope

After all events are streamed, emit a final summary object:

```typescript
interface ValidationSummary {
  type: "summary";
  protocol_version: 1;
  lane: string;
  status: "success" | "failure";
  checks: CheckSummary[];
  duration_ms: number;
  timestamp: string;
}

interface CheckSummary {
  id: string;
  status: "success" | "failure" | "timeout" | "cancelled" | "skipped";
  exit_code: number | null;
  duration_ms: number;
  output_preview?: string; // First 500 chars of output (for failure diagnostics)
}
```

## Implementation Plan (Stage 2)

- [ ] Add `--format=jsonl` to `exohook validate` and `OutputFormat::Jsonl`
- [ ] Implement event emission layer (`MachineOutput`)
- [ ] Emit JSONL events from the pipe runner (stdout/stderr streams)
- [ ] Add summary envelope on completion

## Context Updates (Stage 3)

- [ ] Document machine channel protocol in `docs/manual/features/validation.md`
- [ ] Document JSONL output format in CLI reference

## Drawbacks

- Protocol versioning needs careful backward compatibility.
- JSONL output adds parsing complexity for consumers.

## Alternatives

- Emit a single JSON object at end (not streaming).
- Use a binary protocol (more efficient, less debuggable).

## Unresolved Questions

- Should the protocol include a `check_retry` event for flaky checks?
- Should `idle_warning` be optional by configuration?

## Future Possibilities

- Reserved event types: `check_retry`, `artifact_created`, `lane_skipped`
- Add `protocol_version` negotiation for breaking changes

