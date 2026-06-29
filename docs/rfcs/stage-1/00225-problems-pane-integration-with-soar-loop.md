<!-- exo:225 ulid:01kmzxey1ap2z6e3rtrve703d2 -->

# RFC 225: Problems Pane Integration with SOAR Loop


# RFC 00225: Problems Pane Integration with SOAR Loop

## Summary

Integrate VS Code's Problems pane (diagnostics) into the SOAR workflow to provide instant verification feedback, inform steering decisions, and fill the critical Review phase tool gap identified in RFC 00224.

## Motivation

### The Review Phase Gap

RFC 00224 (The SOAR Loop) identified a critical gap: **the Review phase has 0 dedicated tools**. While the CLI has `exo verify` and `exo criteria` commands, these are not exposed as LM tools, leaving agents without native Review capabilities.

### Untapped Validation Data

VS Code's Problems pane already contains valuable validation information:

- TypeScript/JavaScript errors from language servers
- Rust errors from rust-analyzer
- Linting issues from ESLint, Clippy, etc.
- Custom diagnostics from other extensions

Users invest significant effort configuring these tools. We should leverage this investment rather than duplicating it.

### Instant Feedback vs. Batch Verification

Current workflow:

1. Agent makes changes during Act phase
2. Agent runs `exo verify` (or commits, triggering exohook)
3. Errors discovered after the fact

Proposed workflow:

1. Agent makes changes during Act phase
2. **Problems pane updates in real-time**
3. **Steering adjusts confidence based on error count**
4. Agent addresses errors before attempting verification

This "fail-fast" approach reduces wasted cycles and improves agent efficiency.

## Detailed Design

### Architecture Overview

The design follows the **derived root pattern** established by RFC 00188, which is already the backbone of reactive state in the extension. The key insight is that diagnostics are a **shared perception channel** (RFC 00238): the same data must reach both human (status bar, Problems pane) and agent (`exo-status`, steering, LM tools).

```
VS Code Diagnostics API
        │
        ▼
┌─────────────────────┐
│  DiagnosticsService │  watches onDidChangeDiagnostics
│  (event adapter)    │  fires invalidation on change
└─────────┬───────────┘
          │ invalidateRoots(["derived:diagnostics.summary"])
          ▼
┌─────────────────────────────┐
│  derived:diagnostics.summary │  computed on-demand from VS Code API
│  (DerivedRootRegistry)       │  filtered by severity, grouped by source
└──────┬──────────┬────────────┘
       │          │
       ▼          ▼
   Status Bar   Machine Channel ──► exo-status (CLI)
   (human)      (agent)             exo-steering (CLI)
                                    exo-diagnostics (LM tool)
```

### Design Decision: Event-Driven Derived Roots

Existing derived roots (inbox summary, phase details) read from **file-backed roots** via `scope.read()`. Diagnostics are different — they come from the VS Code API, not from files.

Rather than forcing diagnostics into the file-backed pattern (which the parsimony review correctly flagged as wrong), the design extends ReactivityService with **programmatic invalidation**:

1. `ReactivityService.invalidateRoots(ids: string[])` — new public method that fires `onDidInvalidateRoots`
2. `DerivedRootRegistry` already handles root invalidation — marking cache entries stale
3. The derived root's `compute` function reads from `vscode.languages.getDiagnostics()` directly, not via `scope.read()`

This is a minimal extension: one new method on ReactivityService, and a derived root whose compute function bypasses file-backed roots. The invalidation and caching machinery is unchanged.

### Component 1: DiagnosticsService (Event Adapter)

A lightweight service that bridges VS Code's diagnostic events to the reactive system:

```typescript
// packages/exosuit-vscode/src/services/DiagnosticsService.ts
export class DiagnosticsService implements vscode.Disposable {
  private _disposables: vscode.Disposable[] = [];

  constructor() {
    this._disposables.push(
      vscode.languages.onDidChangeDiagnostics(() => {
        // Invalidate the derived root — next access will recompute
        reactivityService.invalidateRoots(["derived:diagnostics.summary"]);
      }),
    );
  }

  dispose(): void {
    this._disposables.forEach((d) => d.dispose());
  }
}
```

The service is intentionally thin. It does NOT hold state — it simply bridges VS Code events to reactive invalidation. All summarization logic lives in the derived root's compute function.

### Component 2: Derived Root (`derived:diagnostics.summary`)

Registered in `derivedRoots.ts` alongside existing roots:

```typescript
export const DIAGNOSTICS_SUMMARY_ROOT_ID = "derived:diagnostics.summary";

export interface DiagnosticSummary {
  errorCount: number;
  warningCount: number;
  bySource: Record<string, { errors: number; warnings: number }>;
  blocking: boolean; // true if errors > 0
  topErrors: Array<{
    file: string; // workspace-relative path
    line: number;
    source: string;
    message: string;
  }>;
}

function computeDiagnosticSummary(_scope: DerivedScope): DiagnosticSummary {
  const allDiagnostics = vscode.languages.getDiagnostics();
  let errorCount = 0;
  let warningCount = 0;
  const bySource: Record<string, { errors: number; warnings: number }> = {};
  const topErrors: DiagnosticSummary["topErrors"] = [];

  for (const [uri, diagnostics] of allDiagnostics) {
    for (const diag of diagnostics) {
      const source = diag.source ?? "unknown";
      if (!bySource[source]) {
        bySource[source] = { errors: 0, warnings: 0 };
      }

      if (diag.severity === vscode.DiagnosticSeverity.Error) {
        errorCount++;
        bySource[source].errors++;
        if (topErrors.length < 10) {
          topErrors.push({
            file: vscode.workspace.asRelativePath(uri),
            line: diag.range.start.line + 1,
            source,
            message: diag.message,
          });
        }
      } else if (diag.severity === vscode.DiagnosticSeverity.Warning) {
        warningCount++;
        bySource[source].warnings++;
      }
    }
  }

  return {
    errorCount,
    warningCount,
    bySource,
    blocking: errorCount > 0,
    topErrors,
  };
}
```

Registration follows the established pattern:

```typescript
if (!derivedRootRegistry.has(DIAGNOSTICS_SUMMARY_ROOT_ID)) {
  derivedRootRegistry.register({
    id: DIAGNOSTICS_SUMMARY_ROOT_ID,
    compute: computeDiagnosticSummary,
  });
}
```

### Component 3: Shared Perception Outputs

The derived root feeds three outputs — each proving the "shared perception" thesis:

#### Status Bar (Human Perceives)

New `DiagnosticsStatusBarService`, following the InboxStatusBarService pattern:

```typescript
export class DiagnosticsStatusBarService implements vscode.Disposable {
  private statusBarItem: vscode.StatusBarItem;

  constructor() {
    this.statusBarItem = vscode.window.createStatusBarItem(
      vscode.StatusBarAlignment.Left,
      -100, // after phase status
    );

    reactivityService.onDidInvalidateRoots((roots) => {
      if (roots.includes(DIAGNOSTICS_SUMMARY_ROOT_ID)) {
        this.update();
      }
    });

    this.update();
  }

  private update() {
    const summary = derivedRootRegistry.get<DiagnosticSummary>(
      DIAGNOSTICS_SUMMARY_ROOT_ID,
    );

    if (summary.errorCount === 0 && summary.warningCount === 0) {
      this.statusBarItem.hide();
      return;
    }

    const parts: string[] = [];
    if (summary.errorCount > 0) parts.push(`$(error) ${summary.errorCount}`);
    if (summary.warningCount > 0)
      parts.push(`$(warning) ${summary.warningCount}`);
    this.statusBarItem.text = parts.join("  ");
    this.statusBarItem.show();
  }
}
```

#### `exo-status` Enrichment (Agent Perceives)

The machine channel already intercepts `derived:*` requests and returns computed values. When the CLI's `exo status` flows through MachineChannelServer, VS Code can enrich it:

1. **LM tool path** (immediate): Agent calls `exo-status` → tool-factory calls `exo status --format json` via MachineChannelServer → response is enriched with `derived:diagnostics.summary` before returning
2. **CLI enrichment** (future): `exo status` itself could call the machine channel to fetch diagnostic state when running in a VS Code-connected context

For v1, the LM tool path is sufficient. The `exo-status` tool response gains a `diagnostics` field:

```json
{
  "progress_mode": "executing",
  "phase_title": "Shared Perception Channel",
  "diagnostics": {
    "errorCount": 2,
    "warningCount": 5,
    "blocking": true,
    "topErrors": [
      {
        "file": "src/main.ts",
        "line": 42,
        "source": "typescript",
        "message": "Property 'foo' does not exist on type 'Bar'"
      }
    ]
  }
}
```

#### Steering Integration (Self-Model Perceives)

Steering confidence adjusts based on diagnostic state. Since steering is computed by the CLI (`tools/exo/src/steering.rs`), and the CLI doesn't have direct access to VS Code state, the integration path is:

1. **v1**: The `exo-status` LM tool includes diagnostics. Agents see errors and self-steer.
2. **v2**: MachineChannelServer enriches steering responses with diagnostic context before returning them to the LM tool
3. **v3**: Full bidirectional — CLI queries VS Code for diagnostic state during steering computation

For v1, the steering integration is agent-mediated: the agent reads diagnostics from `exo-status`, observes errors exist, and adjusts its behavior. This proves shared perception without requiring architectural changes to the CLI's steering engine.

### Component 4: `exo-diagnostics` LM Tool

Dedicated LM tool for detailed diagnostic queries:

```json
{
  "name": "exo-diagnostics",
  "description": "Get VS Code Problems pane diagnostics for the workspace. Shows errors and warnings from language servers, linters, and other extensions.",
  "parameters": {
    "severity": {
      "type": "string",
      "enum": ["error", "warning", "all"],
      "default": "error"
    }
  }
}
```

Implementation: calls `derived:diagnostics.summary` via machine channel, filters by requested severity.

## Integration with SOAR Phases

| SOAR Phase | Integration                                                                                                        |
| ---------- | ------------------------------------------------------------------------------------------------------------------ |
| **Status** | `exo-status` includes diagnostic summary. Agent sees error count without asking.                                   |
| **Orient** | Agent reads diagnostics, factors into next action choice. Errors suggest "fix first" over "continue implementing." |
| **Act**    | Status bar shows live count. Agent can query `exo-diagnostics` for details after making changes.                   |
| **Review** | `exo-verify` includes diagnostic check. Phase transition can be blocked on errors (via exohook).                   |

## Implementation Plan (Steel Thread)

The implementation is a 3-step steel thread, not horizontal layers. Each step delivers narrow but complete end-to-end value:

| Step | Deliverable                              | Validates                                              | Effort |
| ---- | ---------------------------------------- | ------------------------------------------------------ | ------ |
| 1    | DiagnosticsService + derived root        | Event-driven derived roots work (new reactive pattern) | ~3h    |
| 2    | Status bar + `exo-status` enrichment     | Same data reaches human AND agent (shared perception)  | ~3h    |
| 3    | `exo-diagnostics` LM tool + exohook gate | Agent can query details; phase integrity enforced      | ~3h    |

**Total: ~9h**

Step 1 is the foundation. Step 2 proves the shared perception thesis. Step 3 adds depth.

## Resolved Design Decisions

### 1. Filtering Strategy → Opt-Out

**Decision**: Diagnostics integration is **enabled by default**. Users can opt out.

**Rationale**: A clean Problems pane is a prerequisite for effective steering. Noise is opt-in, not opt-out.

**Axiom**: _"Clean Pane = Clear Mind"_ — Zero-noise Problems pane enables clear steering.

### 2. Blocking Behavior → Hard Block (with exohook)

**Decision**: Phase transitions require clean diagnostics (errors only). Integrated with exohook.

```
exohook pre-phase-finish:
  1. Check uncommitted changes (existing)
  2. Check diagnostics (NEW)
     - If errors > 0 AND block_phase_transition: FAIL
     - Message: "Cannot finish phase: {n} errors in Problems pane"
```

**Rationale**: Aligns with existing phase integrity enforcement. exohook already blocks on uncommitted changes; diagnostics is a natural extension.

### 3. Confidence Adjustment → Heuristics + Power User Knobs

**Decision**: Base penalty per error with configurable thresholds. Zero-config defaults should be solid.

**Default heuristics**:

- `error_penalty = 0.3` — Each error reduces confidence by 30%
- `max_penalty = 0.8` — Floor at 20% confidence (never fully block steering)

**Power user override** in `exosuit.toml`:

```toml
[diagnostics.confidence]
error_penalty = 0.3
max_penalty = 0.8
```

### 4. Severity Scope → Errors Only (v1)

**Decision**: Start with errors only. Warnings as opt-in in v2.

**Rationale**: Warnings are noisy in many codebases. Forcing cleanup is too aggressive for v1. Users who want stricter enforcement can opt in later.

**Future**: Add `severity_filter = "warning"` as power-user opt-in.

## Configuration Schema

```toml
# exosuit.toml
[diagnostics]
enabled = true                # opt-out via false
block_phase_transition = true # require clean pane for phase finish
severity_filter = "error"     # "error" | "warning" | "all"

[diagnostics.confidence]
error_penalty = 0.3           # per-error confidence reduction
max_penalty = 0.8             # floor at 20% confidence
```

## Remaining Questions

### CLI Integration

Should `exo problems` exist as a CLI command, or is this VS Code-only?

The diagnostics API is VS Code-specific, but we could:

- Run language servers in CLI mode
- Parse compiler output
- Integrate with LSP directly

**Deferred**: VS Code-first for v1. CLI integration as future work if needed.

## Alternatives Considered

### 1. Rely on exohook alone

**Rejected**: exohook runs at commit time, too late for iterative feedback.

### 2. Custom diagnostic collection

**Rejected**: Duplicates work users have already done configuring their tools.

### 3. Polling-based approach

**Rejected**: `onDidChangeDiagnostics` event is more efficient and responsive.

## Prior Art

- **Cursor**: Shows errors inline but doesn't integrate with workflow
- **Claude Code**: No diagnostic integration
- **Kiro**: No diagnostic integration

This would be a differentiating feature for Exosuit.

## References

- [RFC 00224: The SOAR Loop](./00224-the-soar-loop-a-workflow-model-for-human-ai-collaboration.md)
- [Shared Perception and Diagnostics Analysis](../../brainstorming/user-flows-agent-analysis.md)
- Idea record: Problems Pane → SOAR Integration (ID: `64ed373e-5f28-421f-9e18-ccaa1d27fcf9`)

## Related RFCs

- RFC 00224: The SOAR Loop — Parent RFC defining the workflow model this integrates with
- RFC 10170: Mutation Boundaries in Feedback Loops — Diagnostics are observe-only; quick fixes are mutations that should follow ODM boundaries
