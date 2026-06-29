<!-- exo:103 ulid:01kg5m2xrghynyn8s818rsy26v -->

# RFC 103: IDE Diagnostics Integration for Instant Verification


> **Note**: This Stage-0 idea has been superseded by [RFC 00225: Problems Pane Integration with SOAR Loop](../stage-1/00225-problems-pane-integration-with-soar-loop.md), which provides a more complete design. This RFC is retained for historical context.

# RFC 0103: IDE Diagnostics Integration for Instant Verification

- **Stage**: 0 (Idea)
- **Champion**: TBD
- **Created**: 2025-07-21
- **Related**: Core Loop, Exohook, Phase Verification

## Summary

Integrate VS Code's Problems pane (diagnostics API) directly into exo's verification loop to provide **instant** (0ms) feedback on code health before slower shell-based checks run.

## Motivation

The current verification loop (`exo verify`, pre-commit hooks) relies on shell commands like `cargo clippy`, `tsc`, and `svelte-check`. These are comprehensive but slow. Meanwhile, the IDE already has real-time diagnostics from language servers that report errors instantly.

**Key insight**: The Problems pane is a leading indicator. If it has errors, the verification will likely fail. If we check it first, we can fail fast and skip expensive shell invocations.

## Proposal

### 1. New LM Tool: `exosuit_get_problems`

Expose a tool that returns the current state of VS Code's Problems pane:

```typescript
interface Problem {
  file: string;
  line: number;
  column: number;
  severity: "error" | "warning" | "info" | "hint";
  message: string;
  source: string; // e.g., "ts", "rust-analyzer", "svelte"
}

// Tool returns
interface GetProblemsResult {
  errors: Problem[];
  warnings: Problem[];
  totalErrors: number;
  totalWarnings: number;
}
```

### 2. Exohook Check Type: Diagnostics

Add a new check type to exohook that queries IDE diagnostics:

```toml
[[checks]]
name = "ide-diagnostics"
input_mode = "diagnostics"
severity_filter = ["error"]  # Only fail on errors, not warnings
sources = ["ts", "rust-analyzer", "svelte"]  # Which language servers to check
```

This runs **before** shell-based checks and fails fast if there are blocking errors.

### 3. Phase Transition Gate

Block phase transitions if the Problems pane has errors:

```
$ exo phase finish
Error: Cannot finish phase - IDE reports 3 errors:
  - src/lib.rs:42: missing lifetime specifier
  - packages/exosuit-vscode/src/extension.ts:15: Property 'foo' does not exist
  - packages/exosuit-vscode/webview/App.svelte:23: Type 'string' is not assignable

Run `exo problems` to see details, or `exo phase finish --force` to override.
```

### 4. CLI Command: `exo problems`

Surface diagnostics in the terminal for non-IDE contexts:

```
$ exo problems
IDE Diagnostics Summary:
  Errors:   3
  Warnings: 12

Errors:
  src/lib.rs:42:5 (rust-analyzer)
    missing lifetime specifier

  packages/exosuit-vscode/src/extension.ts:15:3 (ts)
    Property 'foo' does not exist on type 'Bar'

  packages/exosuit-vscode/webview/App.svelte:23:7 (svelte)
    Type 'string' is not assignable to type 'number'
```

## Benefits

1. **Speed**: Diagnostics are already computed by language servers. Querying them is ~0ms.
2. **DX**: Fail immediately instead of waiting 30+ seconds for full verification.
3. **Alignment**: Makes exo's verification match what the developer already sees in the IDE.
4. **Proactive**: Catches issues before they hit CI.

## Open Questions

1. How do we handle diagnostics in terminal-only workflows (SSH, headless)?
2. Should warnings block phase transitions or just errors?
3. How do we handle flaky/transient diagnostics from language servers?
4. Should this integrate with `exo verify` or be a separate check layer?

## Alternatives Considered

### Watch Mode for CLI Tools

Run `cargo clippy --watch` and `tsc --watch` in the background. Downside: Duplicates what language servers already do.

### Direct Language Server Protocol (LSP) Communication

Query language servers directly instead of through VS Code. More portable but much more complex to implement.

## Next Steps

1. Prototype `exosuit_get_problems` tool in the VS Code extension
2. Test integration with exohook
3. Measure performance impact on phase transitions
4. Gather user feedback on blocking behavior

## Related RFCs

- RFC 00225: Problems Pane Integration with SOAR Loop — Stage-1 RFC that supersedes this idea with a more complete design
- RFC 10170: Mutation Boundaries in Feedback Loops — Diagnostics checking is an observe operation; any auto-fix actions would be mutate operations requiring ODM boundaries
