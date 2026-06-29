<!-- exo:187 ulid:01kmzxey253qp236wywbrnh0vy -->

# RFC 187: Collapse Transitioning Mode into Context-Aware BetweenPhases


# RFC 00187: Collapse Transitioning Mode into Context-Aware BetweenPhases

## Summary

Eliminate the `Transitioning` progress mode. The distinction between "just finished a phase" and "between phases" is not a mode difference—it's a context availability difference. `BetweenPhases` should always show completion context when it exists.

## Motivation

### The False Dichotomy

The current design treats `Transitioning` and `BetweenPhases` as distinct states:

| Mode            | Trigger                                | Shows                           |
| --------------- | -------------------------------------- | ------------------------------- |
| `Transitioning` | All tasks complete, phase not finished | Rich completion context         |
| `BetweenPhases` | No active phase                        | Minimal "start next" navigation |

But these modes answer the **same forward question**: "What phase should I start next?"

The only difference is backward context—and that context **always exists** in the workspace. The mode flag is redundant.

### The Real Problem

After `exo phase finish`:

1. `plan.toml` still contains the completion log
2. The phase's goals and their status are still readable
3. Git history shows what was just committed

Yet the UI drops to a minimal `BetweenPhases` view that ignores all of this.

**The bug isn't mode detection—it's that `BetweenPhases` doesn't read available context.**

## Design

### Simplified Model

```rust
pub enum ProgressMode {
    RoadmapRevision,
    BetweenEpochs,
    BetweenPhases,  // Always context-aware
    Planning,
    Executing,
    Verifying,
    // Transitioning REMOVED
}
```

### BetweenPhases Always Has Context

```rust
pub struct BetweenPhasesContext {
    // The phase that was most recently completed (if any)
    pub completed_phase: Option<CompletedPhaseContext>,

    // The next phase to start (if any)
    pub next_phase: Option<PhasePreview>,

    // Epoch boundary information
    pub epoch_status: EpochBoundary,
}

pub struct CompletedPhaseContext {
    pub phase_id: String,
    pub phase_title: String,
    pub completion_log: String,
    pub goals: Vec<GoalSummary>,
}
```

### Detection Logic

```rust
fn get_completed_phase_context(plan: &Plan) -> Option<CompletedPhaseContext> {
    // Find the most recent completed phase in the active epoch
    let completed_phases: Vec<_> = plan.phases
        .iter()
        .filter(|p| p.status == "completed")
        .collect();

    // Return the last one (most recently completed)
    completed_phases.last().map(|p| CompletedPhaseContext {
        phase_id: p.id.clone(),
        phase_title: p.title.clone(),
        completion_log: p.completion_log.clone().unwrap_or_default(),
        goals: p.goals.clone(),
    })
}
```

**No staleness checks. No time-based decay. No session tracking.**

The context exists until you start the next phase—at which point you're in `Planning` or `Executing`, not `BetweenPhases`.

### Lifecycle

| Event                 | Mode                          | `completed_phase`      |
| --------------------- | ----------------------------- | ---------------------- |
| Working on phase      | `Executing`                   | N/A                    |
| All tasks done        | `Executing` (no pending work) | N/A                    |
| `exo phase finish`    | `BetweenPhases`               | Populated              |
| User returns next day | `BetweenPhases`               | Still populated        |
| `exo phase start`     | `Planning`                    | N/A (new phase active) |

### UI Rendering

`BetweenPhases` view always renders in this order:

1. **Completed Phase Section** (if `completed_phase.is_some()`)
   - Phase title with checkmark
   - Completion log (collapsible)
   - Goals summary

2. **What's Next Section** (always)
   - Next phase preview (if available)
   - "Start Next Phase" action
   - Epoch boundary actions (if applicable)

The "Just Finished" section naturally disappears when you start the next phase—not because of time decay, but because you're no longer in `BetweenPhases`.

### Steering

| State                                 | Primary Actions                     |
| ------------------------------------- | ----------------------------------- |
| `BetweenPhases` + uncommitted changes | Commit & Push, Start Next Phase     |
| `BetweenPhases` + clean               | Start Next Phase, Review Completion |
| `BetweenPhases` + epoch complete      | Finish Epoch, Start Next Phase      |
| `BetweenPhases` + no next phase       | Draft New Phase, Review Backlog     |

## Migration

1. Remove `Transitioning` variant from `ProgressMode` enum
2. Remove `TransitioningContext` type
3. Add `BetweenPhasesContext` with `completed_phase` field
4. Update `derive_progress_mode()` to never return `Transitioning`
5. Merge `buildTransitioningView()` into `buildBetweenStateNavigation()`
6. Delete `buildTransitioningView()`
7. Update all tests

## Rationale

### Why This Works

The key insight: **"When does completion context stop being relevant?"** has a natural answer—**when you start the next phase**.

No arbitrary time thresholds. No session tracking. No marker files. The workspace state machine already handles this.

### Why Not Keep Both Modes?

`Transitioning` was invented to answer "all tasks done but phase not finished." But that's just a special case of `Executing` (or arguably `Verifying`). The real transition happens at `exo phase finish`, which moves you to `BetweenPhases`.

Keeping both modes means:

- Two code paths for similar UI
- Arbitrary "when does Transitioning end?" logic
- User confusion about mode names

### Alignment with Exosuit Axioms

- **Workspace = Truth**: Context is derived from files, not mode flags
- **Parsimony**: 6 states < 7 states
- **No arbitrary thresholds**: Lifecycle events, not time, drive state changes

