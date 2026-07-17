<!-- exo:10120 ulid:01kmzxefdgss2eqg9yeqcz4q12 -->


# RFC 10120: Implementation Plan as Canonical Execution Artifact

- **Superseded by**: RFC 0131


- **Status**: Withdrawn
- **Stage**: 1
- **Reason**:

## Summary

Make `docs/agent-context/current/implementation-plan.toml` the canonical execution artifact for an active phase.

Concretely:

- `plan.goals[]` is the top-level execution structure.
- Each goal can embed `tasks[]`.
- Each task can embed `log[]` (narrative evidence) and `verification[]` (structured verification evidence).
- UI and tooling (CLI + VS Code) treat these embedded objects as the primary source of truth.

Older artifacts become projections/back-compat views during migration (see RFC 10028).

## Motivation

Execution evidence is currently split across multiple files and concepts, which causes drift:

- the plan lives in one place
- tasks live somewhere else
- verification and “what happened” evidence are easy to lose

We want a single place where:

- planned work lives
- execution units live
- evidence attaches directly to the execution units

## Detailed Design

### Canonical file

- `docs/agent-context/current/implementation-plan.toml` is the canonical source for phase execution.

### Shape

Top-level:

- `[phase]` holds phase metadata.
- `[[plan.goals]]` enumerates the execution steps.

Embedded structure:

- `[[plan.goals.tasks]]` enumerates tasks inside a goal.
- `[[plan.goals.tasks.log]]` holds narrative entries.
- `[[plan.goals.tasks.verification]]` holds structured verification entries.

### Status semantics

Task status values are intentionally small and UI-friendly:

- `pending`
- `in-progress`
- `blocked`
- `completed`
- `skipped`

Goal-level “overall status” is derived from task statuses:

- if any task is `blocked` or `in-progress` → overall is “in progress”
- else if all tasks are `completed` → overall is “completed”
- else → overall is “pending”

If a goal has zero embedded tasks, UI SHOULD NOT surface a “No tasks” badge; it should treat it as an unstructured (legacy) goal.

### Verification

Verification entries are append-only and timestamped.

UI guidance:

- “Last verification” is derived from the latest `verification[].when` across tasks in the change.
- If no verification exists, no verification badge is shown.

### Migration rule

During migration:

- `exo task list` and other task projections should prefer embedded tasks when present.
- If no embedded tasks exist, tools may fall back to the legacy source of truth.

## UI Projection Rules (VS Code Studio)

When rendering `implementation-plan.toml`:

- Change cards may show badges:
  - overall status (derived from tasks)
  - last verification (derived from tasks)
- Do not display “No tasks” badges.
- Badges are displayed as neutral/status pills (no emojis).

## Testing

- Unit tests for mapping `plan.goals[].tasks[]` into Studio trees.
- Contract tests ensuring CLI machine channel returns:
  - canonical context paths
  - task lists derived from embedded tasks

## Future Work

- Promote this RFC once the migration is complete and the projection tools (CLI + VS Code) are stable.
- Formalize schemas for `log` and `verification` entries.
