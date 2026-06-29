<!-- exo:232 ulid:01kmzxey1pky9sv1nefaz7t2ym -->

# RFC 232: Situational Awareness: Progressive Context Visualization



# RFC 00232: Situational Awareness: Progressive Context Visualization

- **Superseded by**: RFC 10173


## Summary

The sidebar's Phase Details display currently shows only the micro view (current phase goals and tasks) with no macro context. Users can't see which epoch they're in, what phases are ahead, what just completed, or what's waiting in the inbox. This RFC adds progressive context visualization — showing more or less detail based on workflow state — so users always know where they are relative to the plan.

## Motivation

### Running Blind

Today, the Phase Details sidebar shows:

- Phase title and progress counts
- Goals with nested tasks
- Related RFCs
- Active strike (if any)

It does **not** show:

- Which epoch this phase belongs to
- How many phases are in the epoch and which is current
- What phases come next
- What just completed (orientation breadcrumb)
- What's waiting in the inbox

The result: users feel like they're navigating with a flashlight. They can see the immediate work but have no peripheral awareness of the bigger picture. Every session starts with "where am I?" requiring manual `exo status` / `exo plan` calls.

### Progressive Disclosure

Not all context is equally useful at all times:

- **During execution**: You need focus. Epoch context should be lightweight — a breadcrumb, not a dashboard. The inbox should be a badge count, not a full list.
- **Between phases**: You need the map. Epoch progress should expand. The inbox should surface for triage. Recent context should orient you.

The current UI is binary: either you're in a phase (see tasks) or you're between phases (see navigation). There's no gradient.

### Inbox Invisibility

The inbox (`docs/agent-context/inbox.toml`) is a real concept used constantly — agents add items, steering references them, pending intents surface as badges on goals. But the inbox itself has no dedicated visualization. Items accumulate invisibly until someone thinks to check.

This is especially problematic between phases, when inbox triage should be a natural part of the workflow (see RFC 00231: Chore Phases).

## Detailed Design

### Information Architecture

Three new sections in the Phase Details sidebar, each with two display modes:

#### 1. Epoch Context Section

**During a phase (compact)** — VS Code tree view rendering:

```
▼ Phase: Epoch Context in Phase Details       ✓ 1/3 • 3/9
│
├── 📦 Situational Awareness                      [1/4 phases]
│   ├── ✓ Epoch Context in Phase Details      ← you are here
│   ├── ○ Inbox Queue Visualization
│   ├── ○ Progressive Disclosure & Polish
│   └── ○ ExoSpec: Scaffold and First Namespace
│
├── ▼ Goal: Extend computePhaseDetails... ✓
│   └── [3 tasks completed]
│
├── ▼ Goal: Render epoch context in TreeDataService [0/3]
│   ├── ○ Build epoch header tree item
│   ├── ○ Build sibling phase list items
│   └── ○ Wire into buildPhaseDetailsTreeFromDerived
│
└── ▼ Goal: Adaptive display by progress mode [0/3]
    ├── ○ Compact mode during execution
    ├── ○ Expanded mode between phases
    └── ○ Icon mapping for PhaseStatus values
```

The epoch context section sits **after the phase header, before RFCs and goals** — macro orientation before micro detail.

Shows: epoch title, phase progress fraction, sibling phases as a compact list with status icons.

**PhaseStatus → Icon mapping:**

| PhaseStatus   | Icon             | Color          | Meaning       |
| ------------- | ---------------- | -------------- | ------------- |
| `completed`   | `check`          | `charts.green` | Done          |
| `active`      | `arrow-right`    | `charts.blue`  | Current phase |
| `in-progress` | `arrow-right`    | `charts.blue`  | Current phase |
| `pending`     | `circle-outline` | (default)      | Upcoming      |
| `deferred`    | `circle-slash`   | (default)      | Deferred      |
| `bankrupt`    | `circle-slash`   | `charts.red`   | Bankrupt      |

**Between phases (expanded)**:

```
  📦 Situational Awareness                      [2/4 phases done]
  ├── ✓ Epoch Context in Phase Details
  │     └ 3 goals completed
  ├── ✓ Inbox Queue Visualization
  │     └ 3 goals completed
  ├── ○ Progressive Disclosure & Polish
  │     └ 3 goals planned
  ├── ○ ExoSpec: Scaffold and First Namespace
  │     └ Goals TBD
  ─────
  📦 Next Epoch: (none planned)
```

Shows: epoch title, goal summary per phase, upcoming phase goals preview, next epoch teaser.

#### 2. Inbox Queue Section

**During a phase (badge only)**:

```
📥 Inbox                                        (3)
```

Collapsed by default. Badge shows count. Expands on click to show items.

**Between phases (expanded)**:

```
📥 Inbox                                        (3)
  ⚡ "Review PR #65 comments"          [from: agent]
  💡 "Consider goal-requires-tasks"    [from: user]
  🔧 "Stale branch cleanup"           [from: system]
```

Items grouped by source. Each item has actions: promote to goal, convert to idea, dismiss.

#### 3. Recent Context Breadcrumb

**During a phase**:

```
← Last: "CLI Bug Fixes (Inbox Triage)" completed 2h ago
```

Single line. Shows last completed phase title and relative time. Provides orientation anchor, especially at session start.

**Between phases**:

```
← Just Finished
  Phase: "CLI Bug Fixes (Inbox Triage)"
  Epoch: "Epoch 1: Foundation" (completed)
  Goals: 13 completed, 3 abandoned
  Key output: RFC 228, 229, 230 implemented
```

Expanded summary of what just happened. This already partially exists in the between-phases view ("Just Finished" section) but lacks epoch context and goal summaries.

### Data Model Changes

The `PhaseDetails` type needs new fields:

```typescript
export type PhaseDetails = {
  // ...existing fields...

  /** Epoch context for the active phase */
  epochContext: EpochContext | null;

  /** Inbox summary */
  inbox: InboxSummary;

  /** Recent completion breadcrumb */
  recentContext: RecentContext | null;
};

type EpochContext = {
  epochId: string;
  epochTitle: string;
  phaseIndex: number; // 0-based index of current phase
  totalPhases: number;
  siblingPhases: SiblingPhase[];
  nextEpoch?: { title: string; phaseCount: number };
};

// Full phase status from PhaseStatusSchema — data layer preserves fidelity;
// rendering layer (TreeDataService) maps to display icons/labels.
type PhaseStatus =
  | "pending"
  | "active"
  | "in-progress"
  | "completed"
  | "deferred"
  | "bankrupt";

type SiblingPhase = {
  id: string;
  title: string;
  status: PhaseStatus;
  goalCount: number;
  completionSummary?: string; // first line of completion log
};

type InboxSummary = {
  totalCount: number;
  items: InboxItem[];
};

type InboxItem = {
  id: string;
  title: string;
  source: "user" | "agent" | "system";
  createdAt: string;
};

type RecentContext = {
  phase: { id: string; title: string; completedAt?: string };
  epoch: { id: string; title: string; status: string };
  goalSummary: { completed: number; abandoned: number; total: number };
};
```

### Rendering Strategy

The `computePhaseDetails` function in `derivedRoots.ts` already reads `plan.toml` to find the active phase. Extending it to include epoch context is straightforward — the epoch and sibling phases are already parsed, just not returned.

For inbox, the `derived:inbox` root already exists. Rather than duplicating, `PhaseDetails` can reference the inbox summary or the sidebar can compose both roots.

The progressive disclosure logic lives in the **renderer** (`TreeDataService.ts`), not the data model. The data model always includes full context; the renderer decides how much to show based on `progress.mode`.

### Interaction with Existing RFCs

| RFC                                      | Relationship                                                                             |
| ---------------------------------------- | ---------------------------------------------------------------------------------------- |
| **00184** (Mode-Aware Sidebar)           | This RFC extends 00184's mode-awareness to epoch context and inbox                       |
| **00187** (Context-Aware BetweenPhases)  | "Just Finished" / "What's Next" sections become part of the expanded between-phases view |
| **00185** (Inbox-Driven Sidebar Actions) | Pending-intent badges remain; this adds a dedicated inbox section                        |
| **00227** (Computed Phase Details)       | `derived:phase.details` is the data source we extend                                     |
| **0124** (Async Intent Channel)          | Inbox backend; this RFC adds the visualization layer                                     |
| **00230** (Goals as PER Cycles)          | READY_TO_SHIP mode affects progressive disclosure                                        |
| **00231** (Chore Phases)                 | Inbox triage between phases connects to chore detection                                  |

## Implementation Plan

### Phase 1: Epoch Context in Phase Details

**Goal 1: Data layer** ✅ (commit `160f869`)

- [x] Export `PhaseStatus`/`PhaseStatusSchema` from `@exosuit/core`
- [x] Update `SiblingPhase.status` to use full `PhaseStatus` (not lossy 3-value)
- [x] Add `epochContext?: EpochContext` to `PhaseDetails` type
- [x] Compute epoch context in `computePhaseDetails()` (siblings, phaseIndex, nextEpoch)

**Goal 2: Render epoch context in sidebar** (in progress)

- [ ] Add `buildEpochContextSection()` to `TreeDataService` — returns collapsible section with epoch title, progress description, sibling phase children
- [ ] Add `getPhaseStatusIcon()` — maps `PhaseStatus` × `isCurrentPhase` to `ThemeIcon` with color
- [ ] Insert epoch context section into `buildPhaseDetailsTreeFromDerived()` after header, before RFCs
- [ ] Import `EpochContext`, `SiblingPhase`, `PhaseStatus` types in `TreeDataService`

**Goal 3: Adaptive display** (not started)

- [ ] Compact rendering during execution (epoch name + progress fraction + phase list)
- [ ] Expanded rendering between phases (goal summaries + next epoch teaser)
- [ ] Wire `isBetweenState` check to switch between compact and expanded

### Phase 2: Inbox Queue Visualization

Add inbox section to sidebar:

- Read from `derived:inbox` or `inbox.toml` directly
- Render badge count during phases (collapsed)
- Render full item list between phases (expanded)
- Add item actions (promote, convert, dismiss) — may defer to Phase 3

### Phase 3: Progressive Disclosure & Polish

Wire up display mode adaptation:

- Compact vs expanded rendering based on `progress.mode`
- Recent context breadcrumb (compact during phase, expanded between)
- Smooth transitions between modes
- Ensure performance with large epoch/phase counts

## Alternatives Considered

### Separate Dashboard Panel

Put epoch context and inbox in a dedicated webview panel instead of the sidebar.

**Rejected**: Fragments attention. The sidebar is where the user already looks for phase context. Adding another panel means another thing to check.

### Always-Expanded Context

Show full epoch details even during execution.

**Rejected**: Information overload during focused work. The whole point is progressive disclosure — show what's useful for the current workflow moment.

### Inbox as Separate Sidebar Section

Give inbox its own top-level tree view instead of embedding in Phase Details.

**Partially rejected**: A top-level section could work but risks being ignored (out of sight, out of mind). Embedding it in Phase Details ensures it's visible during the natural orientation flow. Could revisit if the Phase Details section becomes too crowded.

## Prior Art

- **RFC 00184** — Mode-Aware Sidebar Cockpit (progressive disclosure concept)
- **RFC 00187** — Context-Aware BetweenPhases ("Just Finished" / "What's Next")
- **RFC 00185** — Inbox-Driven Sidebar Actions (pending-intent badges)
- **RFC 00227** — Computed Phase Details derived root
- **RFC 0124** — Async Intent Channel (inbox backend)
- **RFC 00230** — Goals as PER Cycles (READY_TO_SHIP mode)
- **RFC 00231** — Chore Phases (inbox triage between phases)

## Unresolved Questions

1. **Epoch count visibility** — With 30+ epochs in plan.toml, should "next epoch" show only the immediate next, or a short list? Probably just the next one to avoid overwhelm.

2. **Inbox item actions** — Should promote/convert/dismiss be inline tree item actions, or require opening a context menu? Inline is faster but clutters the UI.

3. **Stale breadcrumb** — How long should the "recent context" breadcrumb persist? Until the next phase starts? Or fade after N hours?

4. **Performance** — Computing epoch context requires scanning all epochs/phases. With the current plan.toml size (~7000 lines), is this fast enough for reactive updates? Likely yes since it's already parsed, but worth measuring.

## Future Possibilities

- **Epoch health indicators** — Show red/yellow/green based on phase completion rate and time estimates
- **Phase dependency visualization** — Show which phases block others
- **Inbox automation** — System-detected inbox items from CI failures, PR reviews, etc. (connects to RFC 00231)
- **Session timeline** — Show SOAR cycles completed this session for workflow awareness
