<!-- exo:62 ulid:01kg5kp2e1a5dc0edb7p2k5km8 -->

# RFC 62: Protocol/CLI/Tool Test DSL (Template+Holes)


# RFC 0062: Protocol/CLI/Tool Test DSL (Template+Holes)

## Summary

Introduce a single, declarative test-case DSL that can be executed against four runners:

- Unit: raw protocol handler
- Unit: machine-channel adapter (in-process)
- Integration: spawned CLI in human presentation mode (stdout/stderr-aware interleaving)
- Integration: spawned CLI in JSON/tool presentation mode (`--format json`)

The DSL expresses expectations at the semantic level (protocol envelope) and at the presentation level via exact template matching with validated dynamic holes (timestamps/paths/ids/etc).

## Motivation

We want to:

1. Assert protocol semantics are correct (error code, details, steering).
2. Assert tool/CLI surfaces present those semantics correctly.
3. Avoid brittle substring-only tests.
4. Avoid “integration tests” that become useless during refactors because they reuse internal renderers.
5. Support non-deterministic values (timestamps, paths, random ids) without forcing production code into determinism.
6. Make `--format json` reliably machine-consumable.

The project direction (protocol-unified CLI steering: non-ok implies steering) makes these tests especially valuable: it’s easy to regress into “print + Ok(())”.

## Goals

- A single test-case spec generates tests for all runners.
- Integration assertions are exact about structure and content, and fail if output is duplicated or reordered.
- Dynamic segments are supported via typed holes validated by shape constraints.
- Human-mode tests validate both content and which stream (stdout/stderr) emitted each line, including interleaving.
- JSON-mode tests validate that stdout is exactly one JSON value (no extra stdout noise) for both success and failure.

## Non-goals

- Snapshot golden files.
- Full interactive TTY scripting in v1 (streaming/event extensions are planned).
- Code generation outside Rust tests/macro_rules.

## Background: Template + Holes

Expected presentations are templates composed of literal text and holes:

- Literal text must match exactly.
- A hole matches a span and runs a validator (e.g. RFC3339 timestamp, absolute path under sandbox root).

This gives exactness without brittleness and avoids false positives like “printed twice”.

## Design

### DSL Shape

A test case is authored as data:

- `Case`
  - `name`
  - `invocation`: argv + stdin
  - `expected`: envelope semantics + presentation expectations

### Invocation

- `argv`: `Vec<&'static str>` (CLI shape)
- `stdin`: optional string

Presentation mode flags are runner-level, not authored.

### Expected Semantics (Envelope)

- `ok`: bool
- `error_code`: protocol `ErrorCode` when `ok=false`
- `message`: exact string or template+holes
- `details`: JSON structural assertions (subset or exact)
- `steering`:
  - `primary_intent`
  - ordered `next_actions` (label, command, intent, rationale)

Steering ordering is part of the expectation (ordering drift should fail tests).

### Presentation Expectations

We support two presentation channels:

1) Human presentation: line-oriented output with per-line stream tagging
2) JSON/tool presentation: a single JSON envelope (or stream of envelopes later)

The authored case does not provide literal CLI output strings; instead, the harness produces a reference presentation from the expected envelope.

Critically: reference renderers are test-only and must not call production rendering code.

#### Human output model: interleaved streams

Human output is modeled as a single ordered sequence of lines:

- `stream`: stdout | stderr
- `template`: template+holes

Integration runner requirement:

- capture stdout and stderr concurrently
- reconstruct a single interleaved stream
- compare against expected line sequence exactly

This validates both interleaving and stream attribution.

#### JSON reliability contract (`--format json`)

When the JSON/tool runner is used, it invokes the CLI with `--format json` and requires:

- stdout contains exactly one JSON value (object) for the result, optionally followed by trailing whitespace/newline
- no additional non-whitespace text exists on stdout (no banners, progress, duplicates)
- this holds for both success and failure exit statuses

The harness enforces this by parsing the entire stdout buffer as JSON (after trimming). Any trailing non-whitespace content causes a parse failure.

### Dynamic Holes

Initial hole types (extensible):

- RFC3339 timestamp
- UUID
- path under sandbox root
- JSON number/string
- regex (last resort)

Holes are used only when values are truly nondeterministic.

### Reference renderers

- `render_expected_human(&ExpectedEnvelope) -> Vec<ExpectedLine>`
- `render_expected_json(&ExpectedEnvelope) -> ExpectedJson`

These define the canonical contract for tests, but are independent of production formatting.

Mitigation for “same bug twice” risk:

- add a small number of sentinel-propagation tests that do not use the reference renderer.

### Runners

- Unit: raw protocol runner
  - calls `exo::api::handler::handle_request(root, RequestEnvelope)`

- Unit: machine-channel runner
  - calls a minimal extracted in-process adapter equivalent to `exo json channel` framing

- Integration: spawned CLI (human)
  - runs `exo <argv>`
  - captures interleaved stdout/stderr lines and template-matches exactly

- Integration: spawned CLI (json)
  - runs `exo --format json <argv>`
  - parses stdout as a single JSON value and asserts envelope equality
  - enforces JSON reliability contract

### Streaming/event extension (future)

Extend expected outputs from `Single` to `Stream(Vec<Event>)`, and teach runners to compare event streams.

### Sandboxing

Integration runners execute in a sandbox:

- temp dir workspace root
- isolated HOME/config
- fixed cwd

Holes may reference `SANDBOX_ROOT`.

## Inventory Findings (Current Repo)

- Integration tests already use `assert_cmd::cargo::cargo_bin_cmd!("exo")` and `tempfile` sandboxes.
- JSON presentation mode exists and is exercised as `--format json`.
- Machine channel is exercised as `exo json channel` with stdin request envelopes.
- There is an in-process raw protocol handler: `exo::api::handler::handle_request`.
- There is no existing helper for stdout/stderr interleaving capture; `assert_cmd` captures streams separately.

## Implementation Plan

1. Add shared test support module under `tools/exo/tests/support/`.
2. Implement template+holes matcher.
3. Implement stdout/stderr interleaving capture.
4. Implement DSL types and `macro_rules!` generator.
5. Implement four runners.
6. Add the first end-to-end case: `rfc_show_missing`.
7. Run `./scripts/verify-phase.sh`.
8. Definition of done: add one documentation page describing the architecture after implementation stabilizes.

## Decisions

- Machine-channel coverage uses both:
  - a minimal in-process adapter for broad, fast unit coverage
  - a small number of spawned `exo json channel` integration tests to validate the process boundary

- JSON/tool runner uses `--format json` and enforces the JSON reliability contract above.

## Open Questions

- What is the canonical human error format contract (headers, ordering), and do we lock it down immediately or phase it in?
- Where should the shared support module and case declarations live to match repo conventions?

