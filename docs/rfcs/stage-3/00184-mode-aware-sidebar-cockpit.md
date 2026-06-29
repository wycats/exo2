<!-- exo:184 ulid:01kmzxbcxvk5e8mp6zn4bq98y2 -->

# RFC 184: Mode-Aware Sidebar Cockpit


# RFC 00184: Mode-Aware Sidebar Cockpit

- **Supersedes**: RFC 0126

## Summary

Define a mode-aware sidebar cockpit where Phase Details adapts to the 6-state `ProgressMode` workflow and becomes the primary orientation surface for developers, while the Dashboard evolves into a Strategic Overview for between-\* modes only.

## Motivation

- Phase Details does not currently reflect workflow state, so users lose orientation during mode transitions.
- The Dashboard overlaps with Phase Details, causing duplicated information and unclear ownership.
- The extension has no authoritative mode awareness today (local heuristics instead of CLI truth), leading to drift from the CLI state.

## Detailed Design

### 3.1 Terminology

- `ProgressMode`: The 6-state workflow enum from the CLI (`exo status`).
- Cockpit: The primary orientation surface (Phase Details) that adapts to workflow state.
- Strategic Overview: A simplified overview for between-\* modes (replaces the Dashboard).

### 3.2 Mode-Header Mapping

Phase Details is the cockpit. It adapts per mode with a mode-aware header, available actions, and content emphasis. Icons are paired with text (no color-only indicators).

| ProgressMode       | Header text      | Icon         | Available actions                | Content shown                                             |
| ------------------ | ---------------- | ------------ | -------------------------------- | --------------------------------------------------------- |
| `roadmap-revision` | Roadmap Revision | `$(compass)` | Open roadmap, edit epochs        | Strategic Overview: roadmap + epoch list                  |
| `between-epochs`   | Between Epochs   | `$(compass)` | Start epoch, review goals        | Strategic Overview: epoch selection + high-level progress |
| `between-phases`   | Between Phases   | `$(compass)` | Start phase, review goals        | Strategic Overview: phase list + summary                  |
| `planning`         | Planning         | `$(edit)`    | Refine scope, define tasks       | Phase Details: goals, task breakdown, context             |
| `executing`        | Executing        | `$(run)`     | Continue work, complete tasks    | Phase Details: active work, progress, next action         |
| `verifying`        | Verifying        | `$(warning)` | Run verification, review results | Phase Details: verification checklist + results           |

Notes:

- Headers are textual and persistent; icons are supplementary.

### 3.3 Data Flow

Initial:

- On refresh, the extension runs `exo status --json` and extracts `progress_mode`.
- The UI state (cockpit header + content) derives from this `ProgressMode`.
- Cache the `exo status --json` response for 5 seconds, invalidating on canonical semantic root changes.
- Cache the `exo status --json` response via the `DerivedRootRegistry`, invalidating when its dependent roots change.

Future (explicit TODO):

- Replace refresh polling with Machine Channel → Reactivity system integration for live updates.

### 3.4 Phase Details Adaptation

- Mode-aware header: icon + label + progress (text-based status summary).
- For between-\* modes, Phase Details provides a minimal “empty state” navigation that routes users to the Strategic Overview.
- Per-mode action affordances align with the table above (e.g., planning actions during `planning`, verification actions during `verifying`).

### 3.5 Between-Phases UX

Per RFC 00187, `transitioning` is not a distinct mode. The phase-complete decision point is represented as `between-phases` with richer context. The UI surfaces:

1. **Confirmation** — what you accomplished (completed phase summary)
2. **Preview** — what's coming next
3. **Agency** — ability to reorder before starting the next phase

#### 3.5.1 Scenario Detection

The transitioning UI adapts based on epoch position:

| Scenario         | Condition                              | UI Emphasis                                   |
| ---------------- | -------------------------------------- | --------------------------------------------- |
| **Mid-epoch**    | More phases remain in current epoch    | "Up Next" with next phase preview             |
| **Epoch finale** | Last phase in epoch, more epochs exist | "Epoch Complete" banner + upcoming epochs     |
| **Roadmap end**  | Last phase of last epoch               | "Roadmap Milestone" + draft next epoch action |

#### 3.5.2 Transitioning Mode Structure

**Mid-epoch (phases remain):**

```
┌─────────────────────────────────────────┐
│ ▶ Phase Title              ✓ 9/9 • 5/5 │  ← Completed phase
├─────────────────────────────────────────┤
│ 📦 Ready to Ship                        │  ← Mode indicator
│   └─ [Finish Phase]                     │
├─────────────────────────────────────────┤
│ 🔮 Up Next                              │  ← Forward-looking
│   └─ Next Phase Title                   │
│       N goals • RFC XXXXX               │
│       [Preview] [Reorder Phases]        │
├─────────────────────────────────────────┤
│ 📋 Epoch Progress                       │  ← Context
│   └─ Epoch Name: M/N phases             │
│       [View All Phases]                 │
└─────────────────────────────────────────┘
```

**Epoch finale (last phase in epoch):**

```
┌─────────────────────────────────────────┐
│ 🎯 Epoch Complete                       │
│   └─ Epoch Name finished                │
│       [Finish Epoch]                    │
├─────────────────────────────────────────┤
│ 🗺️ Upcoming Epochs                      │
│   └─ Next Epoch 1                       │
│   └─ Next Epoch 2                       │
│       [View Roadmap]                    │
└─────────────────────────────────────────┘
```

**Roadmap end (last epoch, last phase):**

```
┌─────────────────────────────────────────┐
│ 🏆 Roadmap Milestone                    │
│   └─ All planned work complete          │
│       [Finish Epoch]                    │
├─────────────────────────────────────────┤
│ 🌱 What's Next?                         │
│   └─ [Draft New Epoch]                  │
│   └─ [Review Backlog]                   │
└─────────────────────────────────────────┘
```

#### 3.5.3 Data Requirements

Transitioning mode requires additional data from `exo status --json`:

`TransitioningContext` MUST ONLY be present in the JSON payload when `progress_mode == "transitioning"`. When not transitioning, the field MUST be absent (not `null`).

```typescript
interface TransitioningContext {
  // Current epoch info
  epoch_id: string;
  epoch_title: string;
  epoch_phase_count: number;
  epoch_completed_phases: number;

  // Next phase (if mid-epoch)
  next_phase?: {
    id: string;
    title: string;
    goal_count: number;
    rfcs: string[];
  };

  // Upcoming epochs (if epoch finale or roadmap end)
  upcoming_epochs?: Array<{
    id: string;
    title: string;
    phase_count: number;
  }>;

  // Flags
  is_epoch_finale: boolean;
  is_roadmap_end: boolean;
}
```

#### 3.5.4 Implementation Notes

- **CLI Implementation**:
  - Add `TransitioningContext` struct to `tools/exo/src/commands/status.rs`.
  - Add `transitioning_context: Option<TransitioningContext>` to `StatusJson`.
  - Compute only when `progress_mode == ProgressMode::Transitioning`.
- **"Next phase" clarification**: The `next_phase` field refers to the next phase _within the current epoch_, not the next phase globally. If the current phase is the last in its epoch, `next_phase` should be `None` even if other epochs have phases.
- **Extension Integration**:
  - Add `TransitioningContext` interface to `types/progress.ts`.
  - Update `ExoStatusResponse` to include `transitioning_context?: TransitioningContext`.
  - Add `buildTransitioningView()` method to `TreeDataService.ts`.
  - Call from `buildPhaseDetailsTree()` when `progressMode === "transitioning"`.

#### 3.5.5 Acceptance Criteria

- **Mid-epoch scenario**: Shows "Up Next" with next phase preview and an "Epoch Progress" section.
- **Epoch finale scenario**: Shows "Epoch Complete" banner and an "Upcoming Epochs" list.
- **Roadmap end scenario**: Shows "Roadmap Milestone" banner and a "Draft New Epoch" action.
- **All scenarios**: "Finish Phase" action is always visible.

### 3.6 Strategic Overview (Dashboard Evolution)

- A new design from first principles (do not reuse the existing webview code).
- Only visible in: `roadmap-revision`, `between-epochs`, `between-phases`.
- Shows: epoch overview, phase selection, high-level progress.
- Hidden during active phase work (`planning`, `executing`, `verifying`, `transitioning`).
- Visual language will be refined iteratively based on user feedback.

### 3.7 Accessibility

- No color-only indicators; every state uses icon + text.
- All state changes must be readable by screen readers (ARIA labels for icons and headers).
- Header text is the authoritative cue; icons are secondary.

## Technical Specification

### 1. TypeScript Interfaces

```typescript
// packages/exosuit-vscode/src/types/progress.ts

/**
 * Progress mode enum matching CLI's ProgressMode.
 * Serialized as kebab-case in JSON.
 */
export type ProgressMode =
  | "roadmap-revision"
  | "between-epochs"
  | "between-phases"
  | "planning"
  | "executing"
  | "verifying"
  | "transitioning";

/**
 * Status response from `exo status --json`.
 */
export interface ExoStatusResponse {
  phase_id?: string;
  phase_title?: string;
  epoch_title?: string;
  progress_mode: ProgressMode;
  pending_tasks: number;
  completed_tasks: number;
}
```

### 2. StatusService Specification

```typescript
// packages/exosuit-vscode/src/services/StatusService.ts

/**
 * Service for fetching and caching CLI status.
 * - Caches via DerivedRootRegistry
 * - Invalidates when derived root dependencies change
 * - Singleton pattern
 */
class StatusService {
  getProgressMode(): Promise<ProgressMode>;
  getStatus(): Promise<ExoStatusResponse>;
  invalidate(): void;
}
```

### 3. TreeDataService Changes

In `buildPhaseDetailsTree()`:

- Add `progressMode: ProgressMode` parameter
- Call new `buildModeAwareHeader(progressMode, phaseTitle, progress)` method
- For between-\* modes, return `buildBetweenStateNavigation(progressMode)` instead of normal tree

### 4. Mode-Header Mapping (Resolved)

| ProgressMode       | Header             | Icon           |
| ------------------ | ------------------ | -------------- |
| `roadmap-revision` | "Roadmap Revision" | `$(compass)`   |
| `between-epochs`   | "Between Epochs"   | `$(compass)`   |
| `between-phases`   | "Between Phases"   | `$(compass)`   |
| `planning`         | "Planning"         | `$(edit)`      |
| `executing`        | "Executing"        | `$(run)`       |
| `verifying`        | "Verifying"        | `$(warning)`   |
| `transitioning`    | "Ready to Ship"    | `$(check-all)` |

## Implementation Plan

1. Phase 1: Add `ProgressMode` data flow (CLI → extension on refresh via `exo status --json`).
2. Phase 2: Mode-aware Phase Details header.
3. Phase 3: Empty state navigation for between-\* modes.
4. Phase 4: Strategic Overview (new design from first principles).
5. Phase 5: Transitioning Mode UX (forward-looking context based on epoch position).

Note: Visual language will be refined iteratively based on user feedback.

## Supersession Notes

- Partially supersedes RFC 0053 (Modal Workflows): keeps the context-adaptive concept but removes the dual-sidebar approach.
- Evolves RFC 0126 (Dashboard V2): Dashboard becomes Strategic Overview with a new design.

## Drawbacks

- Adds a CLI call on refresh (mitigated by future reactivity integration).
- Requires new Strategic Overview design work.

## Alternatives

- Keep Dashboard as-is (rejected: overlaps with Phase Details).
- Deprecate Dashboard entirely (rejected: strategic modes need an overview surface).

## Unresolved Questions

- Strategic Overview visual design details (iterative refinement required).
- Refresh frequency optimization for `exo status --json`.
