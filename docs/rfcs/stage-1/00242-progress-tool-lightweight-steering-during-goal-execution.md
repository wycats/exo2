<!-- exo:242 ulid:01kmzxey1tx2y3dea6cvdx76kr -->

# RFC 242: Progress Tool: Task Logs as Lightweight Steering


# RFC 00242: Progress Tool: Task Logs as Lightweight Steering {#00242}

## Summary

Add `exo task log` — a command that writes a progress entry to a task's `log[]` array in implementation-plan.toml and returns lightweight steering. This closes the gap between task-start and task-complete, giving the agent (and user) intermediate visibility and course-correction opportunities.

Progress entries use the same `[[plan.goals.tasks.log]]` schema defined by RFC 0131 and read by RFC 0148's walkthrough aggregation. No new storage format is needed.

## Motivation

From [user-flows.md](../../brainstorming/user-flows.md):

> "We likely want an additional 'progress' tool that the agent can use to report progress (which would show up in the sidebar so the user could see it), which would also give us another opportunity to provide contextual steering to the agent."

The current model has two steering touchpoints during execution:

1. **Task completion** (`exo task complete --log`) — runs validation, returns steering
2. **Implicit** — the agent just... continues

This creates a gap: if the agent is on the wrong track, we don't detect it until they try to mark a task done (potentially after significant wasted effort).

`exo task log` closes this gap with an inner steering loop that's lighter than task-complete but still returns validation and guidance.

## Design

### CLI Command

```
exo task log <task-id> --message "Refactored validation to use new hook schema"
```

Writes a `[[plan.goals.tasks.log]]` entry:

```toml
[[plan.goals.tasks.log]]
kind = "progress"
message = "Refactored validation to use new hook schema"
when = "2026-02-12T22:15:00Z"
```

The `kind` field distinguishes progress entries from completion entries:

- `"progress"` — intermediate checkpoint (written by `exo task log`)
- `"completion"` — terminal entry (written by `exo task complete --log`)
- `"note"` — general narrative (written manually or by other tools)

### Steering Return

Returns the same shape as task-complete but runs "quick" validation (scoped to changed files, no full test suite):

```json
{
  "validation": {
    "problems": { "error": 0, "warning": 2 }
  },
  "steering": {
    "next_actions": [...],
    "pending_intents": {
      "gate": [],        // Empty during task execution (gates apply at scope exit)
      "reminder": [...], // Goal-scoped intents presented as checkpoint (RFC 10174)
      "untriaged": [...]
    }
  }
}
```

The `pending_intents.reminder` array contains intents attached to the current goal — these are presented as "nag" checkpoints per RFC 10174's hierarchical queue model.

### Sidebar Visibility

The latest `log[]` entry for each goal is shown in the Phase Details sidebar tree:

- Displayed as a description line on the goal item (e.g., `0/4 tasks • Refactored validation...`)
- Timestamp provides recency awareness
- Accumulates in storage (walkthrough needs the full history), but sidebar shows only the latest

This addresses user-flows.md's concern that progress should be visible without reading the chat transcript.

### LM Tool

Exposed as `exo-task-log` in command-spec:

```json
{
  "name": "exo-task-log",
  "description": "Log a progress message on a task",
  "parameters": {
    "id": { "type": "string", "description": "Task ID" },
    "message": { "type": "string", "description": "Progress message" }
  }
}
```

The capability tree already declares this as `phase.execution.task.append_log`.

## Resolved Questions

These were open in the original Stage 0 version and are now resolved:

1. ~~**Storage**: Persisted or ephemeral?~~ → **Persisted** in implementation-plan.toml as `[[plan.goals.tasks.log]]`. This is what makes the walkthrough work — RFC 0148 aggregates these entries into the narrative.

2. ~~**Sidebar UX**: Accumulate or replace?~~ → **Accumulate** in storage (walkthrough needs history). **Show latest only** in sidebar (avoids tree bloat).

## Open Questions

1. **Frequency**: How often should the agent call `exo task log`? This is an agent instruction concern, not a tool design concern. The tool should be available; when to call it is guidance. A reasonable default: after each logical chunk of work (e.g., after implementing a function, after fixing a test).

## Relationship to Other RFCs

- **RFC 0131** (Implementation Plan): Defines the `log[]` schema this command writes to
- **RFC 0148** (Implicit Walkthrough): Reads these entries as walkthrough narrative
- **RFC 00229** (Goal Status Authority): Progress doesn't change status; task-complete does
- **RFC 10174** (Inbox System): Defines the hierarchical intent queue. Task-log is a **presentation opportunity** for goal-scoped intents — when `exo task log` returns steering, it should include `pending_intents.reminder` for intents attached to the current goal. This creates natural "nag" checkpoints without manual scheduling.
