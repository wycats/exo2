<!-- exo:231 ulid:01kmzxefeyps50fjek1z6xawxn -->

# RFC 231: Chore Phases: Automated Interstitial Work


# RFC 00231: Chore Phases: Automated Interstitial Work

## Summary

Some work doesn't fit the goal/PER model: merging PRs, rebasing branches, triaging inbox items, reviewing completed epochs. This work needs to happen but doesn't warrant a full phase with goals, tasks, and PER ceremony. Chore phases are lightweight interstitial phases — real phases (same infrastructure, same tracking) but with reduced ceremony and system-detected triggers.

## Motivation

### The Interstitial Gap

Between phases, housekeeping accumulates:
- Open PRs need merge, rebase, or conflict resolution
- Completed epochs need formal review
- Stale branches need cleanup
- Inbox items accumulated during execution
- Dependency updates flagged by CI

Currently, this work either:
1. **Gets ignored** — PRs pile up, epochs go unreviewed
2. **Gets shoehorned into the next phase** — polluting its goals with unrelated cleanup
3. **Gets done ad-hoc** — outside the tracking system, invisible to history

### Why Not a Separate Concept?

The temptation is to model chores as something other than phases — a parallel track, a queue, a special mode. But fragmentation is a silent killer. Every new concept adds cognitive load, requires its own tooling, and creates integration seams.

Chores are phases. They use the same infrastructure. The difference is ceremony weight, not kind.

## Detailed Design

### Chore Phases

A chore phase is a phase with `kind = "chore"` (default phases have `kind = "regular"`). Differences:

| Aspect | Regular Phase | Chore Phase |
|--------|--------------|-------------|
| Goals | Multiple, PER-sized | Typically 1, implicit |
| PER ceremony | Full (Prepare→Execute→Review) | Light (just do it) |
| Trigger | User planning | System detection |
| Duration | Multiple sessions | Single session (time-boxed) |
| PR | Usually produces one | May not need one |

If a chore takes more than a session, it should be **promoted to a regular phase** — the work was bigger than expected and deserves full tracking.

### Chore Sources

The system can detect when chores exist and surface them in steering:

- **Open PRs** — `gh pr list` shows PRs needing attention
- **Unreviewed epochs** — Completed epochs without review
- **Stale branches** — Branches with no recent commits
- **Inbox overflow** — Inbox items above a threshold
- **Failed CI** — Builds that need attention

Each source is a function: `() → Option<ChoreDescription>`. Steering queries all sources between phases.

### Steering Integration

```
Phase N finishes
       │
       ▼
  ┌─────────────┐
  │ Steering:    │  "You have 2 open PRs and an unreviewed epoch.
  │ Chores       │   Handle these before starting Phase N+1?"
  │ detected     │
  └──────┬──────┘
         │
    ┌────┴────┐
    │ Chore   │  exo phase start --chore "Merge open PRs"
    │ phase   │
    └────┬────┘
         │
         ▼
  Phase N+1 planning
```

### Dedicated Commands

To minimize ceremony, chore phases get streamlined commands:

- `exo chore start "description"` — Creates and starts a chore phase in one step
- `exo chore done` — Finishes the chore phase with minimal logging
- `exo chore skip` — Acknowledges the chore but defers it (prevents re-nagging)

These are sugar over `exo phase start --kind chore` / `exo phase finish`, not a separate system.

### Promotion

If a chore reveals unexpected complexity:

```
exo chore promote  →  Converts chore phase to regular phase
                      Adds goals, enables full PER ceremony
```

This is the escape hatch. Start light, escalate if needed.

## Relationship to RFC 00230

RFC 00230 defines READY_TO_SHIP as the phase-level state when all goals are complete but the PR isn't merged yet. Chore phases are what happens *after* READY_TO_SHIP → SHIPPED, before the next regular phase begins.

The flow:
```
Regular Phase (EXECUTING → READY_TO_SHIP → SHIPPED)
       │
       ▼
  Chore detection (steering queries chore sources)
       │
       ▼
  Chore Phase(s) if needed
       │
       ▼
  Next Regular Phase (PLANNING → EXECUTING → ...)
```

## Alternatives Considered

### Chores as a Parallel Track

Model chores as a separate queue alongside phases.

**Rejected**: Fragmentation. Two tracking systems means two sets of tooling, two mental models, and inevitable gaps where they don't integrate.

### Chores as Steering-Only

Just have steering suggest actions without creating phases.

**Rejected**: Invisible work. If it's not tracked, it didn't happen. Chore phases leave a record in phase history.

### No Chore Concept

Let users create regular phases for housekeeping.

**Partially rejected**: This works but adds unnecessary ceremony. The `kind = "chore"` flag is a small addition that enables significant UX streamlining.

## Prior Art

- **RFC 00230** — Goals as PER Cycles, READY_TO_SHIP mode
- **RFC 00229** — Goal Status Authority (abandoned state is analogous to chore skip)
- **GTD** (David Allen) — "2-minute rule": if it takes less than 2 minutes, do it now. Chores are the workflow equivalent.

## Unresolved Questions

1. **Chore scope** — Should chores be limited to system-detected triggers, or can users create them manually?

2. **Chore history** — Should chore phases appear in phase history the same as regular phases, or be visually distinguished?

3. **Multi-chore batching** — If steering detects 5 chores, should they be one chore phase or five? Probably one with multiple implicit goals.

4. **Epoch placement** — Do chore phases belong to the current epoch, or are they epoch-less?

## Future Possibilities

- **Chore automation** — Some chores (merge clean PRs, delete merged branches) could be fully automated with user approval
- **Chore metrics** — Track time spent on chores vs. regular work for workflow health
- **Chore prevention** — If certain chores recur, suggest process changes to prevent them

