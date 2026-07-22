<!-- exo:229 ulid:01kmzxefd7k7vbg4578vwdvcpe -->

# RFC 229: Goal Status Authority: plan.toml as Single Source with Derived Signals

- **Superseded by**: RFC 10176

- **Status**: Withdrawn
- **Stage**: 1
- **Reason**:

# RFC 00229: Goal Status Authority: plan.toml as Single Source with Derived Signals

## Summary

Establish `plan.toml` as the **single authoritative source** for goal status, with `implementation-plan.toml` task completion providing **derived signals** that inform steering—not competing truth.

## Motivation

### Current Problem: Dual Sources of Truth

The codebase has two places where goal status can be determined:

1. **Explicit status** in `plan.toml`:

   ```toml
   [[epoch.phase.goals]]
   id = "rfc228-type-renames"
   status = "in-progress"  # ← Explicit, set by `exo goal complete`
   ```

2. **Derived status** from `implementation-plan.toml`:
   ```rust
   // derived.rs computes status from task completion
   DerivedGoalStatus {
       status: "completed",  // ← All tasks done
       reason: "all 6 tasks completed"
   }
   ```

Different commands read from different sources:

- `exo goal list` → reads explicit status from plan.toml
- `exo phase status` → overlays derived status
- `exo-status` (LM tool) → uses derived status

**Result**: Goal shows "pending" in one view, "completed" in another. Confusion and bugs.

### Root Cause

Derivation was added to provide accurate progress without requiring manual updates. But it became a _competing_ truth rather than an _input signal_.

## Detailed Design

### Architecture: Authority + Signals

```
┌──────────────────────────────────────────────────────────────┐
│  plan.toml                                                   │
│  ────────────                                                │
│  status: "in-progress"    ← AUTHORITATIVE (single source)   │
│  completion_log: None                                        │
└──────────────────────────────────────────────────────────────┘
          │
          ▼
┌──────────────────────────────────────────────────────────────┐
│  derived.rs (computed once, read by steering)                │
│  ──────────                                                  │
│  DerivedGoalStatus {                                         │
│    tasks_complete: true,   ← from implementation-plan.toml   │
│    reason: "all 6 tasks done"                                │
│  }                                                           │
└──────────────────────────────────────────────────────────────┘
          │
          ▼
┌──────────────────────────────────────────────────────────────┐
│  steering.rs                                                 │
│  ───────────                                                 │
│  if derived.tasks_complete && goal.completion_log.is_none(): │
│    → suggest "exo goal complete <id> --log '...'"            │
│    → high confidence, intent: "record"                       │
└──────────────────────────────────────────────────────────────┘
```

### Key Invariant

```
goal.status == "completed" ⟺ (all_tasks_done ∧ completion_log.is_some())
goal.status == "abandoned" ⟺ completion_log.is_some()
```

A goal is only "completed" when:

1. All implementation tasks are done
2. The human has logged what was learned/accomplished

**The completion log is not bookkeeping—it's the final deliverable.**

### State Machine

```
                              steering fires:
                             "Log completion"
                                    │
pending ──▶ in-progress ──▶ [tasks done] ──▶ completed
                │                                (log required,
                │                                 tasks must be done)
                │
                └──▶ abandoned
                     (log required,
                      tasks need NOT be done)
```

Both terminal states (`completed` and `abandoned`) require a log message.

- `completed`: All tasks must be done. The log records what was accomplished.
- `abandoned`: Tasks need not be done. The log records _why_ the goal was closed (superseded, no longer relevant, etc.)

There's no explicit `tasks-complete` state. Instead:

- Status stays `in-progress` until explicitly completed or abandoned
- Steering detects (all tasks done + no log) and prompts action
- `exo goal complete` requires a log message **and** all tasks done
- `exo goal abandon` requires a log message but **not** task completion

### Changes Required

#### 1. Demote DerivedGoalStatus

Change from "competing truth" to "input signal":

```rust
// Before: Used as authoritative status
pub struct DerivedGoalStatus {
    pub status: String,      // ← Competed with plan.toml
    pub reason: String,
}

// After: Used as signal to steering
pub struct DerivedGoalStatus {
    pub tasks_complete: bool, // ← Signal, not status
    pub pending_count: usize,
    pub completed_count: usize,
    pub reason: String,
}
```

#### 2. Update Steering

Add action when tasks done but goal not logged:

```rust
// steering.rs
if derived.tasks_complete && goal.completion_log.is_none() {
    actions.push(SteeringAction {
        command: format!("exo goal complete {} --log '...'", goal.id),
        confidence: 0.95,
        intent: "record",
        label: "Log goal completion",
        rationale: format!(
            "All {} tasks for '{}' are done. Log what was accomplished to close.",
            derived.completed_count, goal.label
        ),
    });
}
```

#### 3. Require Log for Completion

```rust
// goal.rs - complete command
pub fn complete(id: &str, log: Option<String>) -> Result<()> {
    let log = log.ok_or_else(|| {
        anyhow!("Completion log required. Use: exo goal complete {} --log 'What was accomplished'", id)
    })?;
    let signal = derive_goal_signal(id)?;
    if signal.pending_count > 0 {
        anyhow::bail!(
            "Cannot complete goal '{}': {} tasks still pending. Resolve them or abandon the goal with a log.",
            id,
            signal.pending_count
        );
    }
    // ...
}
```

#### 3b. Abandon a Goal (Terminal State)

Use a dedicated command to close a goal without finishing its tasks:

```
exo goal abandon <id> --log '...'
```

- Requires a log message (same requirement as `complete`)
- Does **not** require all tasks to be done
- Sets `goal.status = "abandoned"` in plan.toml

##### Derived Display Behavior

- When a goal is `abandoned`, its pending tasks **display** as "abandoned" in derived views (CLI, VS Code tree)
- Completed tasks under an abandoned goal remain displayed as completed
- The task records in `implementation-plan.toml` are **not** mutated

#### 4. Remove Status Override in Views

Commands that currently overlay derived status should instead:

- Show authoritative status from plan.toml
- Add a "tasks: 6/6 done" progress indicator separately

### Migration

1. Existing goals with `status: "completed"` are valid
2. Goals where (tasks done + no log) get steering prompts
3. No schema changes required

## Implementation Phases

### Phase 1: Refactor DerivedGoalStatus

Change the struct from "competing status" to "signal for steering":

| File         | Change                                                                                   |
| ------------ | ---------------------------------------------------------------------------------------- |
| `derived.rs` | Replace `status: String` with `tasks_complete: bool`, `pending_count`, `completed_count` |
| `derived.rs` | Rename function to `derive_goal_signal()` (optional, for clarity)                        |

### Phase 2: Fix Authority in CLI Views

Make all CLI commands read goal status from plan.toml only:

| File           | Change                                                    |
| -------------- | --------------------------------------------------------- |
| `goal.rs`      | `GoalList` uses plan.toml status, not derived overlay     |
| `task.rs`      | Task list shows plan.toml status, derived signal separate |
| `phase_cmd.rs` | JSON + human output use plan.toml status                  |

### Phase 3: Fix WorldState

The `exo status` command should read from plan.toml:

| File        | Change                                                     |
| ----------- | ---------------------------------------------------------- |
| `status.rs` | `WorldState.goals` populated from plan.toml, not impl-plan |
| `status.rs` | `pending_goals`/`completed_goals` count from plan.toml     |

### Phase 4: Enforce Completion Log

Prevent bypassing the log requirement:

| File      | Change                                                            |
| --------- | ----------------------------------------------------------------- |
| `goal.rs` | `exo goal complete` requires `--log` parameter                    |
| `goal.rs` | `exo goal complete` refuses when any tasks are still pending      |
| `impl.rs` | `exo impl update-status` cannot set goal to "completed" directly  |
| `goal.rs` | Add `exo goal abandon <id> --log '...'` (no task-completion gate) |

### Phase 5: Update Steering

Add the "log completion" prompt:

| File          | Change                                                                       |
| ------------- | ---------------------------------------------------------------------------- |
| `steering.rs` | When `tasks_complete && !completion_log` → suggest "exo goal complete --log" |

### Phase 6: Update VS Code Tree

| File               | Change                                                    |
| ------------------ | --------------------------------------------------------- |
| `PhaseView.svelte` | Use plan.toml status for display, derived signal as badge |
| `PhaseView.svelte` | Only show completion log when plan status is "completed"  |
| `PhaseView.svelte` | Display abandoned goals and derived "abandoned" tasks     |

### Phase 7: Cleanup

| File                  | Change                                      |
| --------------------- | ------------------------------------------- |
| `impl.rs`             | Remove legacy `goal_completions` write path |
| `lm_tool_metadata.rs` | Update descriptions to clarify authority    |

## Success Criteria

- [ ] `plan.toml` status is the only source of truth
- [ ] `DerivedGoalStatus` refactored to signal-oriented fields
- [ ] Steering prompts "log completion" when tasks done
- [ ] `exo goal complete` requires `--log` parameter
- [ ] All views show consistent status from plan.toml
- [ ] Task progress shown separately from goal status

## Alternatives Considered

### Auto-Sync Status

Automatically update plan.toml status when tasks complete.

**Rejected**: Bypasses the human synthesis step. The log is the point.

### Add `tasks-complete` State

Explicit intermediate state in the schema.

**Rejected**: Adds complexity. The condition (tasks done + no log) is clear enough for steering to detect.

### Remove Derivation Entirely

Only use explicit status, never compute from tasks.

**Rejected**: Loses the useful signal. Steering needs to know when to prompt.

### Task Cascade on Abandonment

Auto-mutate pending tasks to `skipped` when a goal is abandoned.

**Rejected**: Keeps source data honest; the derived layer handles display. The tasks were never "skipped"—the goal was abandoned out from under them.

## Prior Art

- RFC 00228 established Goal/Task terminology
- RFC 00224 (SOAR Loop) establishes the Review phase pattern—logging is the Review artifact

## Unresolved Questions

1. Should `--log` be positional or require the flag?
2. Should steering show the prompt only once, or persistently until logged?
3. How should this interact with strike goals (ad-hoc work)?
