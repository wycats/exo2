<!-- exo:10173 ulid:01kmzxey1w2348ydhb191hzwc4 -->


# RFC 10173: Phase Details: Unified Derived Root and Progressive Context

> Supersedes: RFC 00227, RFC 00232

> Consolidates RFC 00227 (Computed Phase Details: Unified Derived Root) and RFC 00232 (Situational Awareness: Progressive Context Visualization) into a single document covering the Phase Details system's data architecture and UX design.

## Summary

The Phase Details sidebar is the primary workspace for understanding the current phase. This RFC defines two aspects of that system:

1. **Data Architecture** — A `derived:phase.details` computed root that provides a unified, trace-validated phase details structure consumed by the sidebar, CLI, and LM tools. _(Implemented.)_
2. **Progressive Context** — Epoch context, inbox visibility, and recent context breadcrumbs that expand between phases and contract during execution. _(Partially implemented — epoch context exists; inbox section and breadcrumbs are forward plan.)_

## Part 1: Unified Derived Root

\*Origin: RFC 00227. **Status: Implemented.\***

### Motivation

Phase details were previously computed in multiple places with different logic:

- **Phase Details sidebar** (`TreeDataService.ts`): Parsed `plan.toml` and `implementation-plan.toml` directly
- **CLI** (`phase_cmd.rs`): Read the same files, computed similar structure
- **LM tools**: Called CLI, got output

This caused duplicated logic, no caching, and inconsistency risk.

### The `derived:phase.details` Root

A derived root (per RFC 00188) that computes the full phase details structure:

```typescript
interface PhaseDetails {
  phase: {
    id: string;
    title: string;
    epochId: string;
    epochTitle: string;
    rfcs: string[];
  } | null;

  progress: {
    mode: ProgressMode;
    goalsCompleted: number;
    goalsTotal: number;
    tasksCompleted: number;
    tasksTotal: number;
  };

  goals: Array<{
    id: string;
    title: string;
    status: "pending" | "in-progress" | "completed" | "skipped";
    kind?: "strike";
    startedAt?: string;
    completionLog?: string[];
    tasks: Array<{
      id: string;
      title: string;
      status: "pending" | "in-progress" | "completed" | "skipped";
      completionLog?: string[];
      derivedStatus?: { status: string; reason: string };
    }>;
  }>;

  activeStrike?: {
    goalId: string;
    title: string;
    startedAt: string;
  };

  epochContext: EpochContext | null;

  contextCheck: {
    implementationPlan: "found" | "missing";
    gitDirty: boolean;
  };

  verification: {
    automated: string[];
    manual: string[];
  };
}
```

### Dependencies

The derived root depends on canonical semantic state:

- canonical plan state — active epoch, active phase, goal definitions, phase status, epoch structure
- canonical phase-details/task state — goal/task hierarchy, task progress, task execution details

These values may be transported into the extension through legacy root identifiers during migration, but the semantic source of truth is SQLite-backed canonical state, not TOML artifacts. The derived root must revalidate based on canonical reads and their traces, not on projection file existence.

### Canonical ProgressMode Derivation

`PhaseDetails.progress.mode` must be derived from canonical semantic state, not from whether an implementation artifact exists.

The derivation is:

1. `between-epochs` if there is no active epoch
2. `between-phases` if there is an active epoch but no active phase
3. `executing` if there is an active phase and any goal or task in that phase is `in-progress`
4. `planning` otherwise when there is an active phase

This makes the mode a summary over current work state rather than a proxy for file materialization. In particular, `executing` must not be inferred from the presence of `implementation-plan.toml` or any equivalent projection.

### Not Materialized

The derived root lives in memory (cached), is invalidated when sources change, and is recomputed on-demand. It is **not** written to disk.

### Hybrid MachineChannel Access

The CLI and LM tools access this extension-hosted value via the Hybrid MachineChannel architecture:

1. `MachineChannelServer` intercepts requests targeting `derived:*` namespace
2. Routes to `DerivedRootRegistry` for in-memory resolution
3. Bypasses CLI subprocess — computation happens once, centrally

The CLI continues to work without the extension by computing from canonical semantic state directly.

### Resolved Questions

**Should the derived root include steering?** No. Steering depends on a much wider surface (git status, inbox, snapshots). Separate `derived:steering` root per RFC 00188.

**How does "between phases" work?** `progress.mode` includes `"between-phases"` with `phase: null`. One derived root handles both states.

**`phase.summary` vs `phase.details`?** Use existing `derived:status.summary` for lightweight consumers; `derived:phase.details` for full hierarchical data.

## Part 2: Progressive Context

_Origin: RFC 00232. **Status: Partially implemented** — epoch context section exists; inbox section and recent context breadcrumb are forward plan._

### Motivation

The Phase Details sidebar shows only the micro view (current phase goals and tasks) with no macro context. Users can't see which epoch they're in, what phases are ahead, or what's waiting in the inbox. The result: every session starts with "where am I?" requiring manual `exo status` calls.

### Progressive Disclosure Principle

Not all context is equally useful at all times:

- **During execution**: Focus. Epoch context is a breadcrumb, inbox is a badge count.
- **Between phases**: The map. Epoch progress expands, inbox surfaces for triage.

The progressive disclosure logic lives in the **renderer** (`TreeDataService.ts`), not the data model. The data model always includes full context; the renderer decides how much to show based on `progress.mode`.

### Epoch Context Section (Implemented)

**During a phase (compact)**:

```
📦 Situational Awareness                      [1/4 phases]
├── ✓ Epoch Context in Phase Details      ← you are here
├── ○ Inbox Queue Visualization
├── ○ Progressive Disclosure & Polish
└── ○ ExoSpec: Scaffold and First Namespace
```

Sits after the phase header, before RFCs and goals. Shows epoch title, phase progress fraction, sibling phases with status icons.

**PhaseStatus → Icon mapping:**

| PhaseStatus              | Icon             | Color          | Meaning       |
| ------------------------ | ---------------- | -------------- | ------------- |
| `completed`              | `check`          | `charts.green` | Done          |
| `active` / `in-progress` | `arrow-right`    | `charts.blue`  | Current phase |
| `pending`                | `circle-outline` | (default)      | Upcoming      |
| `deferred`               | `circle-slash`   | (default)      | Deferred      |
| `bankrupt`               | `circle-slash`   | `charts.red`   | Bankrupt      |

**Between phases (expanded)**: Shows goal summaries per phase, upcoming phase goals preview, next epoch teaser.

### Epoch Context Data Model (Implemented)

```typescript
type EpochContext = {
  epochId: string;
  epochTitle: string;
  phaseIndex: number;
  totalPhases: number;
  siblingPhases: SiblingPhase[];
  nextEpoch?: { title: string; phaseCount: number };
};

type SiblingPhase = {
  id: string;
  title: string;
  status: PhaseStatus;
  goalCount: number;
  completionSummary?: string;
};
```

### Forward Plan: Inbox Queue Section (Not Implemented)

**During a phase (badge only)**:

```
📥 Inbox                                        (3)
```

**Between phases (expanded)**: Full item list grouped by source, with actions (promote to goal, convert to idea, dismiss).

### Forward Plan: Recent Context Breadcrumb (Not Implemented)

**During a phase**: Single line showing last completed phase title and relative time.

**Between phases**: Expanded summary of what just happened — epoch context, goal summaries, key outputs.

### Visual Mockup: During Execution

```
┌─────────────────────────────────────────────────────┐
│ PHASE DETAILS                                       │
├─────────────────────────────────────────────────────┤
│                                                     │
│ ▶ Pipeline Polish                    [3/5 • 12/18]  │  ← Phase header
│                                                     │
│ ─────────────────────────────────────────────────── │
│                                                     │
│ 📦 Situational Awareness                    [1/4]   │  ← Epoch context (existing)
│ ├── ✓ Epoch Context in Phase Details                │
│ ├── → Pipeline Polish                  ← you are here
│ ├── ○ Inbox Queue Visualization                     │
│ └── ○ ExoSpec: Scaffold                             │
│                                                     │
│ ─────────────────────────────────────────────────── │
│                                                     │
│ 📥 Inbox                                      (3)   │  ← Inbox badge
│                                                     │
│ 🕐 Last: RFC Pipeline Visibility        2 hours ago │  ← Recent breadcrumb
│                                                     │
│ ─────────────────────────────────────────────────── │
│                                                     │
│ ▼ Recently Completed (2)                            │
│   ├── ✓ Surface migration                           │
│   └── ✓ Movement arrows (superseded)                │
│                                                     │
│ ◔ Implement inbox queue section                     │  ← Active goal
│   └── ◌ Wire inbox data to tree                     │
│                                                     │
│ ○ Implement recent context breadcrumb               │  ← Pending goal
│                                                     │
│ ─────────────────────────────────────────────────── │
│                                                     │
│ ▶ Coming Up                                         │
│   ├── Next: Inbox Queue Visualization (4 goals)    │
│   └── Then: ExoSpec epoch...                        │
│                                                     │
└─────────────────────────────────────────────────────┘
```

### Visual Mockup: Between Phases

```
┌─────────────────────────────────────────────────────┐
│ PHASE DETAILS                          [no active]  │
├─────────────────────────────────────────────────────┤
│                                                     │
│ 🎉 Just Finished: Pipeline Polish                   │  ← Expanded breadcrumb
│    Completed 5 goals • 18 tasks • 2 hours ago       │
│    Key outputs: Surface migration, RFC consolidation│
│                                                     │
│ ─────────────────────────────────────────────────── │
│                                                     │
│ 📥 Inbox (3 items)                                  │  ← Expanded inbox
│ ├── 💡 from steering: "Consider TDD for next phase" │
│ │      [Promote] [→ Idea] [Dismiss]                 │
│ ├── 📝 from session: "Breadcrumb stale timeout?"    │
│ │      [Promote] [→ Idea] [Dismiss]                 │
│ └── 🔧 from friction: "Improve CLI output alignment"│
│        [Promote] [→ Idea] [Dismiss]                 │
│                                                     │
│ ─────────────────────────────────────────────────── │
│                                                     │
│ 📦 Situational Awareness                    [2/4]   │
│ ├── ✓ Epoch Context in Phase Details                │
│ ├── ✓ Pipeline Polish                               │
│ ├── → Inbox Queue Visualization        ← up next    │
│ └── ○ ExoSpec: Scaffold                             │
│                                                     │
│ ─────────────────────────────────────────────────── │
│                                                     │
│ ▶ Ready to Start                                    │
│   Run 'exo phase start' to begin next phase         │
│                                                     │
└─────────────────────────────────────────────────────┘
```

## Related RFCs

- **RFC 00188** — Derived Roots & Reactive Caches (foundation)
- **RFC 10172** — Sidebar Visual Design (visual treatment of tree items)
- **RFC 00185** — Inbox-Driven Sidebar Actions (inbox backend; this RFC adds visualization)
- **RFC 00242** — Task Logs as Steering (progress visibility in Phase Details)
- **RFC 00238** — Pipeline-Aware Self-Model (pipeline as organizing principle)

## Unresolved Questions

1. **Inbox item actions** — Inline tree item actions vs. context menu? Inline is faster but clutters the UI.
2. **Stale breadcrumb** — How long should the recent context breadcrumb persist?
3. **Performance** — Computing epoch context requires scanning all epochs/phases. With large `plan.toml` files, is this fast enough for reactive updates?
