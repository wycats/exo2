<!-- exo:10170 ulid:01kmzxbcy5rxzf5523p5j8scpy -->


# RFC 10170: Mutation Boundaries in Feedback Loops

## Summary

This RFC defines a principled model for handling the interaction between **observation** (running checks, gathering diagnostics) and **mutation** (editing files, running fixes) in the Exosuit feedback loop. Feedback loops involving both observation and mutation require explicit **atomicity boundaries** to ensure observations reflect stable state.

## Motivation

### The Problem

The Exosuit system involves multiple feedback loops:

1. **Exohook validation**: Checks run on file changes, producing pass/fail results
2. **Diagnostic reporting**: LSP-style diagnostics with optional quick fixes
3. **Agent steering**: The agent updates tasks/goals/phases, receives steering feedback
4. **Continuous run**: Auto-revalidation on file save

These loops have a fundamental tension:

- **Observation should be idempotent** — running a check twice on the same state should give the same result
- **Mutation creates new state** — which invalidates prior observations
- **Some checks ARE mutations** — formatters, auto-fixers, code generators
- **Quick fixes are mutations triggered by observations** — clicking a diagnostic fix edits files

Without explicit boundaries, we get race conditions and confusion:

- Agent reads check results while checks are still running
- Continuous run triggers mid-edit, producing spurious failures
- A formatting check runs, changes files, invalidates other checks' results
- Quick fix applied while diagnostics are stale

### The Human IDE Analogy

Humans deal with this intuitively in IDEs:

- Save file → linter runs → see red squiggles
- Click "quick fix" → file edited → linter re-runs
- Don't click quick fix while linter is still running

The IDE enforces some atomicity (quick fix waits for diagnostics to settle), and humans self-regulate (don't frantically click while updating). Agents need explicit rules for what humans do implicitly.

### The Contextual Injection Philosophy

A key Exosuit design principle: **tools should be designed so that using them naturally produces steering as a side effect**.

The agent doesn't need to remember "check for guidance"—it just follows sensible workflow patterns (report progress, update tasks, run checks), and contextual information flows in organically. This is why:

- **`exo` tools return steering**: Running `exo status` or `exo task complete` doesn't just update state—it returns contextual guidance about what to do next.
- **Exohook exists**: Validation checks are a sensible workflow step that agents will follow because the pattern is simple. But now they're getting rich, contextual information about project health without a "giant dump in an instruction file."
- **Test Explorer integration matters**: It creates an IDE-native representation that can be projected into agent perception through the same contextual mechanisms.
- **Shared perception**: Users have already configured their IDEs to present rich information contextually. Exosuit can project that user perception into the agent's "consciousness" as part of normal flows.

**Mutation boundaries aren't just about preventing bad states**—they're about creating natural pause points where contextual information can flow. A commit point is a moment where the agent will naturally interact with a tool, and that tool can provide rich context about what just happened and what should happen next.

## Design

### Check Categories

Checks should declare their **mutation behavior**:

```toml
[checks.lint]
command = "eslint --max-warnings 0 {{files}}"
category = "observe"  # Pure observation, no file changes

[checks.format]
command = "prettier --write {{files}}"
category = "mutate"   # Changes files as primary purpose

[checks.typecheck]
command = "tsc --noEmit"
category = "observe"

[checks.codegen]
command = "prisma generate"
category = "mutate"
```

| Category  | Idempotent | Mutates Files | Safe in Continuous Run |
| --------- | ---------- | ------------- | ---------------------- |
| `observe` | Yes        | No            | Yes                    |
| `mutate`  | No         | Yes           | No                     |

In `CheckV3`, the `category` field replaces the legacy `fix` field.

`category` describes what the check _does_, not when it should run. A `mutate` check still runs in pre-commit hooks; the category just informs the system that it modifies files, enabling proper concurrency guards and invalidation. The workflow-level `fix_policy` controls when auto-fix runs and is orthogonal to `category`.

### The Observe-Decide-Mutate (ODM) Loop

All feedback loops follow this pattern:

```
┌─────────────────────────────────────────────────────────┐
│                                                         │
│  ┌─────────┐    ┌─────────┐    ┌─────────┐             │
│  │ OBSERVE │───▶│ DECIDE  │───▶│ MUTATE  │─────────────┘
│  └─────────┘    └─────────┘    └─────────┘
│       │              │              │
│       ▼              ▼              ▼
│   Run checks    Review results   Edit files
│   Gather diags  Choose action    Run fixes
│   Read state    Plan next step   Update artifacts
│                                         │
│                                         ▼
│                                  ┌─────────────┐
│                                  │ COMMIT POINT│
│                                  │ (stable)    │
│                                  └─────────────┘
└─────────────────────────────────────────────────────────┘
```

**Key principle**: Steering and observation should happen at **commit points**, not mid-mutation.

### Continuous Run Semantics

Continuous run should only include `observe` category checks:

```toml
[workflows.dev]
label = "Dev (uncommitted)"
checks = ["lint", "typecheck", "format", "test"]

# Only observe checks run in continuous mode
continuous_checks = ["lint", "typecheck", "test"]  # excludes format
```

Alternatively, derive this automatically from check categories.

### Mutation Checks as Explicit Actions

`mutate` category checks should:

1. **Not run in continuous mode** — only on explicit request
2. **Show a confirmation** — "This check will modify files. Proceed?"
3. **Invalidate prior observations** — after running, all `observe` checks should re-run
4. **Block concurrent checks** — no other checks run while a mutating check is in progress

### Quick Fixes and Diagnostics

Diagnostics with quick fixes follow the same model:

```typescript
interface Diagnostic {
  message: string;
  severity: "error" | "warning" | "info";
  location: Location;
  fixes?: QuickFix[]; // Optional mutations
}

interface QuickFix {
  label: string;
  edits: TextEdit[]; // The mutation
  isPreferred?: boolean;
}
```

Applying a quick fix is a **mutation** that:

1. Should wait for current observations to complete
2. Should trigger re-observation after applying
3. Should be atomic (all edits or none)

### Agent Steering Boundaries

For the agent feedback loop:

1. **Observe**: Run `exo status`, read diagnostics, check validation results
2. **Decide**: Analyze results, plan next action, update task status
3. **Mutate**: Edit files, run fixes, update plan artifacts
4. **Commit**: All mutations complete, state is stable

**Steering should be read at commit points**, not mid-mutation. This means:

- Don't read steering while file edits are in progress
- Don't start new edits while checks are running
- Treat "check running" as a transient state, not a steering input

### Terminology and Scope

The Exosuit system uses multiple loops at different scopes:

| Loop                                | Scope               |
| ----------------------------------- | ------------------- |
| **SOAR** (Status-Orient-Act-Review) | Project/session     |
| **ODM** (Observe-Decide-Mutate)     | Phase/task          |
| **PER** (Prepare-Execute-Review)    | Goal/implementation |

**ODM** names the observe/decide/mutate boundary inside work execution, **SOAR** frames project- and session-level flow, and **PER** structures goal- or implementation-level work. These loops are intentionally layered: each one describes the same work at a different scope.

## Implementation

### Implemented: Check Categories

1. Added `CheckCategory` enum (`observe`, `mutate`) to `exohook`
2. Added `category` field to `CheckV3`, removing `fix: bool` (hard break)
3. Added migration plugin to convert `fix = true` → `category = "mutate"` in existing configs
4. Replaced `CheckPlan.autofix: bool` with `CheckPlan.category: CheckCategory`
5. Updated `should_fix()` and scheduling logic to use `category` instead of `fix`/`autofix`
6. Updated v2→v3 migration to write `category` instead of `fix`
7. Defaulted to `observe` if not specified

### Implemented: Continuous Run Filtering

1. Added `category` to JSONL discovery output (`DiscoveryItem::Check`)
2. TypeScript Test Explorer parses and stores `category` in `ItemMeta`
3. `findMatchingLanes()` filters out `mutate` checks from continuous run
4. Backward compatible: undefined category (old CLI) treated as allowed

### Planned: UI Enhancements

1. Add UI indication that some checks are "manual only"
2. Add explicit "Run All" vs "Run Continuous" distinction

### Planned: Mutation Boundaries

1. Add concurrency guard: no checks run while mutation in progress
2. Add invalidation: mutation completes → re-run observe checks
3. Add confirmation UI for mutation checks

### Planned: Unified Diagnostic Model

1. Exohook check failures become diagnostics
2. Quick fixes from checks (e.g., "run formatter") surface as diagnostic actions
3. Agent can "apply fix" through same mechanism as human

## Open Questions

1. **Granularity**: Should mutation boundaries be per-check or per-file?
2. **Partial mutations**: What if a mutation check partially succeeds?
3. **Rollback**: Should we support undoing mutation checks?
4. **Parallelism**: Can multiple `observe` checks run in parallel? (Probably yes)
5. **Agent autonomy**: Should agents be able to auto-apply "safe" fixes without confirmation?
6. **`fix_command` retention**: Should `fix_command` remain as an override for `mutate` checks, or should it be folded into a more general "command variants" pattern?

## Related Work

- VS Code's "format on save" has similar atomicity concerns
- Git's staging area is a mutation boundary
- Database transactions provide atomicity guarantees
- The OODA loop (Observe-Orient-Decide-Act) is a related military/business concept

## Related RFCs

- RFC 00224: The SOAR Loop — ODM refines Act/Review transitions
- RFC 00240: Fractal SOAR / Goal Loop — mutation boundaries clarify when steering can happen inside L1 loops
- RFC 0026: Validation-Based Reactivity — atomic snapshots and transaction boundaries map to observe/mutate
- RFC 0119: Reactive File System — watcher-driven invalidation relates to commit points
- RFC 00225: Problems Pane Integration — diagnostics are observe-only, quick fixes are mutations

## Appendix: The SOAR Connection

This RFC's ODM loop relates to the existing SOAR loop:

| SOAR   | ODM        | Notes                             |
| ------ | ---------- | --------------------------------- |
| Status | Observe    | Both gather current state         |
| Orient | Decide     | Both synthesize and plan          |
| Act    | Mutate     | Both execute changes              |
| Review | (implicit) | ODM's commit point enables review |

The key addition is explicit **mutation boundaries** and **check categories** that SOAR doesn't address.
