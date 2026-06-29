<!-- exo:10027 ulid:01kmzxbcyyz9xaps73bgmp67hv -->



# RFC 10027: Operation-Context Errors and Boundary Conversion

## Summary

Introduce a consistent error propagation strategy that preserves *semantic operation context* (what we were trying to do) while maintaining Exosuit invariants:

- **Non-ok implies steering** across all surfaces.
- In `--format json`, **stdout is exactly one JSON value**.
- Failures are **protocol-shaped**: stable `error.code`, human-oriented `error.message`, structured `error.details`, and explicit steering.

This RFC standardizes:

1. Where and how internal errors are converted into `ExoFailure`.
2. A minimal, stable schema for `error.details` that retains actionable context.
3. A renderer policy that can recover `ExoFailure` even when wrapped (e.g. `anyhow::Error`).

## Motivation

Leaf errors (`std::io::Error`, parse errors, subprocess failures) often lack the *nouns and verbs* a human needs. Propagating them unadorned leads to low-signal messages like “EPERM on stat call”.

We instead want errors like:

- “Couldn’t read the plan manifest.”
- “Verification failed running scripts/verify-phase.sh.”

…while still preserving technical causes for debugging.

## Design

### A. Semantic operation context

Any error that crosses a **surface boundary** must carry a semantic operation label and relevant artifacts (paths, commands, runners). The operation context is the layer that still knows the user-facing nouns.

Examples of `op`:

- `"read plan manifest"`
- `"parse implementation plan"`
- `"execute verify runner"`

### B. Conversion points (“boundaries”)

Internal implementation may use `anyhow` or typed internal errors. Conversion to `ExoFailure` occurs at the first layer that:

- knows the semantic operation (`op`), and
- can attach appropriate steering.

Boundaries include:

- CLI dispatch (human and `--format json`)
- Machine-channel handler (protocol envelopes)
- Any other externally-consumed API surfaces

### C. `error.details` schema (minimal)

We standardize a small, boring, stable schema for internal failures:

```json
{
  "op": "read plan manifest",
  "path": "docs/agent-context/plan.toml",
  "runner": "scripts/verify-phase.sh",
  "exit_code": 1,
  "causes": [
    { "message": "Permission denied (os error 13)" },
    { "message": "..." }
  ]
}
```

Rules:

- Only include fields that apply (`path`, `runner`, `exit_code`, etc.).
- `causes` is an ordered chain from outermost to innermost.
- Do **not** embed verbose debug/backtrace in JSON by default.

### D. Renderer recovery policy

The CLI renderer must recover an embedded `ExoFailure` even when wrapped by error aggregation types (notably `anyhow::Error`).

Rationale: This avoids losing structured `details` + steering and prevents accidental fallback to fabricated steering.

### E. Steering policy

When converting to `ExoFailure`, steering should be authored at the same layer as the operation context.

- “Next” actions are the primary path.
- “Repair” actions are used when remediation is likely.

## Implementation Notes

- Add renderer support for detecting `ExoFailure` inside `anyhow::Error`.
- Add a helper to capture the error cause chain into `error.details.causes` for fallback/internal errors.
- Migrate at least one phase-critical command path to rely on renderer recovery rather than per-command downcasting.

## Risks / Tradeoffs

- Over-serialization: dumping too much detail into JSON harms stability and may leak information.
- Under-serialization: too little detail reduces debugging value.

This RFC biases toward a small, stable `details` schema and leaves deeper diagnostics to logs.

