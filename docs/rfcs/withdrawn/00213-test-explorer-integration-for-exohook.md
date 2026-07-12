<!-- exo:213 ulid:01kmzxefd32j45gf2aa598tcbb -->

# RFC 213: Test Explorer Integration for Exohook

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**:

# RFC 00213: Test Explorer Integration for Exohook

## Summary

Integrate exohook validation checks with VS Code's Test Explorer, allowing users to discover, run, and view results of validation checks through the standard testing UI. This requires defining the conceptual mapping between exohook's domain model (workflows, checks, triggers) and Test Explorer's model (test suites, test items), as well as the machine-readable protocol needed to drive the integration.

## Motivation

### Current State

1. **Exohook** provides validation lane execution for git hooks with real-time progress feedback. It runs "checks" organized into "lanes" (soon to be "workflows"), with support for parallel execution, file filtering, and autofix capabilities.

2. **VS Code Test Explorer** is currently wired to phase tasks (via `TestControllerService`), not exohook. It shows phase tasks as test items and runs them via `exosuit.verifyTask`.

3. **The gap**: Users cannot discover or run exohook checks through Test Explorer. They must use the terminal (`exohook validate <lane>`), losing the benefits of:
   - Visual test hierarchy
   - Click-to-run individual checks
   - Integrated result display with pass/fail indicators
   - Cancellation support
   - Re-run failed checks

### Why Test Explorer?

Test Explorer is the natural home for validation checks because:

1. **Familiar UX**: Developers already use Test Explorer for unit tests
2. **Discoverability**: Checks become visible without reading config files
3. **Selective execution**: Run one check, one workflow, or everything
4. **Result persistence**: See which checks passed/failed at a glance
5. **Integration**: Works with VS Code's testing keybindings and commands

### Use Cases

1. **Pre-commit validation**: Run the `coherence` workflow before committing
2. **Pre-push gate**: Run the `gate` workflow before pushing
3. **Targeted fixes**: Re-run just the failing `clippy` check after fixing warnings
4. **CI preview**: See what CI will run via the `ci` workflow

## Problem Statement

To integrate exohook with Test Explorer, we need to answer:

1. **What is a "test"?** Not all checks are tests in the traditional sense. Some are formatters (`cargo fmt`), some are linters (`clippy`), some are actual test runners (`cargo test`).

2. **How do we structure the hierarchy?** Test Explorer supports nested items. Should we show:
   - Flat list of all checks?
   - Workflows as suites containing checks?
   - Something else?

3. **What output format does exohook need?** Test Explorer needs:
   - Enumeration of test items (discovery)
   - Run capability with progress
   - Result reporting (pass/fail/skip)
   - Cancellation support

4. **How does cancellation work?** Exohook currently lacks a cancellation protocol. Long-running checks need graceful termination.

5. **How do we handle non-test checks?** Formatters and linters have different semantics:
   - Formatters may modify files (autofix)
   - Linters produce diagnostics, not pass/fail
   - Some checks are "informational" (coverage reports)

## Possible Solutions

### Solution A: Checks as Tests, Workflows as Suites

Map directly to Test Explorer's model:

```
Test Explorer
├── gate (workflow)
│   ├── cargo-fmt (check)
│   ├── cargo-clippy (check)
│   ├── cargo-test (check)
│   └── pnpm-test (check)
├── coherence (workflow)
│   ├── cargo-fmt (check)
│   └── eslint (check)
└── ci (workflow)
    └── ... (checks)
```

**Pros**: Natural mapping, familiar hierarchy
**Cons**: Checks may appear in multiple workflows (duplication), formatters aren't really "tests"

### Solution B: Flat Check List with Tags

Show all checks as a flat list, use tags/labels for filtering:

```
Test Explorer
├── cargo-fmt [formatter, coherence, gate]
├── cargo-clippy [linter, gate]
├── cargo-test [test, gate, ci]
└── pnpm-test [test, gate, ci]
```

**Pros**: No duplication, clear check identity
**Cons**: Loses workflow context, harder to "run all gate checks"

### Solution C: Hybrid with Check Categories

Group by check type, then show workflow membership:

```
Test Explorer
├── Tests
│   ├── cargo-test
│   └── pnpm-test
├── Linters
│   ├── cargo-clippy
│   └── eslint
└── Formatters
    └── cargo-fmt
```

**Pros**: Semantic grouping, clear purpose
**Cons**: Doesn't match how users think about validation (by workflow/trigger)

### Solution D: Workflow-First with Deduplication

Show workflows as suites, but deduplicate checks that appear in multiple:

```
Test Explorer
├── gate (workflow)
│   ├── cargo-fmt → (shared)
│   ├── cargo-clippy
│   └── cargo-test
└── coherence (workflow)
    └── cargo-fmt → (shared with gate)
```

**Pros**: Preserves workflow context, shows relationships
**Cons**: Complex UI, may confuse users

## Protocol Requirements

Regardless of hierarchy choice, exohook needs machine-readable output for:

### 1. Discovery

```bash
exohook discover --format=jsonl
```

Output:
```jsonl
{"type":"workflow","id":"gate","label":"Gate","checks":["fmt","clippy","test"]}
{"type":"check","id":"fmt","label":"Format","command":"cargo fmt --check","category":"formatter"}
{"type":"check","id":"clippy","label":"Clippy","command":"cargo clippy","category":"linter"}
{"type":"check","id":"test","label":"Test","command":"cargo test","category":"test"}
```

### 2. Execution with Progress

RFC 0113 (Machine Channel Protocol) already defines this:
- `check_started`, `check_output`, `check_completed` events
- JSONL streaming format

### 3. Cancellation

New requirement: exohook needs to handle `SIGINT`/`SIGTERM` gracefully:
- Stop spawning new checks
- Send termination signal to running checks
- Emit `check_completed` with `status: "cancelled"`
- Emit `lane_completed` with accurate counts

### 4. Result Mapping

| Exohook Status | Test Explorer State |
|----------------|---------------------|
| `success` (exit 0) | Passed |
| `failure` (exit non-0) | Failed |
| `timeout` | Failed (with message) |
| `cancelled` | Skipped |
| `skipped` (fail-fast) | Skipped |

## Open Questions

### Conceptual

1. **Should formatters appear in Test Explorer at all?** They modify files, which is side-effectful. Maybe they belong in a separate "Fix" action?

2. **How do we handle checks with `autofix = true`?** Should Test Explorer offer a "Fix" action alongside "Run"?

3. **What about checks that produce diagnostics?** Linters output warnings/errors. Should these integrate with VS Code's Problems panel?

4. **Should we show checks that aren't in any workflow?** Orphan checks exist in config but aren't triggered.

### Technical

5. **How does the extension discover checks?** Options:
   - Parse `hooks.toml` directly (TypeScript)
   - Call `exohook discover --format=jsonl` (subprocess)
   - Use a WASM build of exohook config parser

6. **How do we handle workspace-relative paths?** Checks run from repo root, but Test Explorer items need URIs.

7. **What's the cancellation mechanism?** Options:
   - Send SIGINT to exohook process
   - Use a control channel (stdin command)
   - Implement a cancel file/socket

8. **How do we handle parallel vs sequential execution?** Test Explorer can run tests in parallel. Should we respect exohook's `parallel` setting or let Test Explorer control it?

### UX

9. **What icon/badge should formatters vs linters vs tests have?** Visual distinction helps users understand check types.

10. **Should running a workflow run all its checks, or just the ones not already passing?** (Incremental vs full validation)

11. **How do we show check output?** Options:
    - Test Explorer's built-in output panel
    - Exosuit's output channel
    - Integrated terminal

## Dependencies

- **RFC 0113** (Machine Channel Protocol): Provides the JSONL streaming format for execution
- **RFC 0212** (Hooks Config Ergonomics): The "workflows/checks/triggers" model affects hierarchy design

## Future Possibilities

1. **Diagnostic integration**: Linter output → Problems panel
2. **Code lens**: "Run check" links in `hooks.toml`
3. **Autofix actions**: "Fix" button for formatter checks
4. **Coverage integration**: Show coverage from `cargo llvm-cov` in editor
5. **Watch mode**: Re-run checks on file save
6. **CI preview**: Show which checks would run in CI before pushing

## Non-Goals (for this RFC)

- Replacing the terminal-based `exohook validate` experience
- Full CI workflow generation (covered by RFC 0140)
- Real-time streaming in Test Explorer (may not be supported by API)
