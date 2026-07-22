<!-- exo:177 ulid:01kmzxbczsf1cw4wfqm6pkw316 -->

# RFC 177: Goals and Tasks: Unified Work Item Model

- **Superseded by**: RFC 10176

- **Status**: Withdrawn
- **Stage**: 1
- **Reason**:

# RFC 00177: Goals and Tasks: Unified Work Item Model

## Summary

Replace the current dual-concept system (tasks in plan.toml, steps in implementation-plan.toml) with a unified hierarchy: **Goals** (planning units) contain **Tasks** (execution units). Task completion logs accumulate into goal completion narratives, which form the walkthrough.

## Motivation

### Current Problem

We have two similar concepts with no clear relationship:

- **Tasks** in `plan.toml` under phases (planning-time)
- **Steps** in `implementation-plan.toml` (execution-time)

This causes:

1. Phase Details pane shows 0 tasks (reads impl-plan, expects steps, finds none)
2. `exo task list` and `exo phase status` show different data
3. No clear command flow for "what do I update when?"
4. Walkthrough has no clear source

### Root Cause

"Task" and "step" are too similar. We never defined their relationship or lifecycle.

## Design

### Hierarchy

```
Phase
└── Goal (planning unit, lives in plan.toml)
    └── Task (execution unit, lives in implementation-plan.toml)
```

### Key Principles

1. **Goals are planning artifacts** - created before phase starts, linked to RFCs
2. **Tasks are execution artifacts** - created during phase execution, belong to goals
3. **Logs are required** - task completion requires a log message
4. **Walkthrough = goal completion logs** - human-authored narratives, not auto-generated

### Data Ownership

**`plan.toml`** is the persistent record. Goal metadata lives here and survives phase transitions.

**`implementation-plan.toml`** is the ephemeral working document. It references goals by ID and contains phase-scoped execution details (tasks). It is reset when a new phase starts.

| Data                              | File                       | Lifespan     | Created By                |
| --------------------------------- | -------------------------- | ------------ | ------------------------- |
| Goal ID, label, RFC link          | `plan.toml`                | Permanent    | `exo goal add`            |
| Goal status                       | `plan.toml`                | Permanent    | `exo goal complete`       |
| Goal completion log               | `plan.toml`                | Permanent    | `exo goal complete --log` |
| Goal reference (ID only)          | `implementation-plan.toml` | Phase-scoped | `exo phase start`         |
| Task ID, description, status, log | `implementation-plan.toml` | Phase-scoped | `exo task add/complete`   |

**Anti-pattern**: Duplicating metadata from `plan.toml` in `implementation-plan.toml`. This creates drift between files.

This applies at **both levels**:

- **Phase metadata**: `implementation-plan.toml` stores only `phase.id` (a FK into `plan.toml`). Phase title and RFC linkage are **not** duplicated — consumers derive them via `plan.toml` or computed roots like `derived:phase.details`.
- **Goal metadata**: `implementation-plan.toml` stores only goal IDs as references. Goal title, status, and completion_log live exclusively in `plan.toml`.

**Correct pattern**: `implementation-plan.toml` contains only IDs as foreign keys. Readers join on `plan.toml` for full metadata.

### Command Flow

**Planning (before phase start):**

```bash
exo goal add "Add impl commands" --rfc 0062
exo goal add "Add LM tools" --rfc 0135
```

**Execution (during phase):**

```bash
exo task add "Add list subcommand" --goal impl-aliases
exo task complete add-list --log "Thin wrapper over plan inspection"
exo goal complete impl-aliases --log "All three implemented as wrappers"
```

### RFC Linkage

Goals should link to an RFC (or RFC section). If no RFC:

- Warning on creation: "No RFC linked. Goal must be self-documenting."
- Steering may nudge to add RFC or expand description

### Phase Details Tree (UI)

```
▼ Goal: Add impl commands [2/3] RFC:0062
  ├── ✓ Add list subcommand
  │     "Thin wrapper over plan inspection"
  ├── ✓ Add show subcommand
  │     "Reused existing formatter"
  └── ○ Add status subcommand

▼ Goal: Add impl commands ✓ RFC:0062
  │  Log: "All three implemented as wrappers over plan inspection"
  └── [3 tasks completed]
```

### Schema Changes

**plan.toml:**

```toml
[[epochs.phases.goals]]  # replaces [[epochs.phases.tasks]]
id = "impl-aliases"
label = "Add exo impl list/show/status commands"
rfc = "0062"  # optional, strongly encouraged
status = "pending"
```

**implementation-plan.toml:**

```toml
[phase]
id = "phase-id-here"

[plan]

[[plan.goals]]
id = "impl-aliases"  # Reference to plan.toml — NO title/status/completion_log here

[[plan.goals.tasks]]
id = "add-list"
title = "Add list subcommand"
status = "completed"
log = "Thin wrapper over plan inspection"

[[plan.goals]]
id = "add-lm-tools"  # Another goal reference

[verification]
automated = ["Run scripts/verify-phase.sh"]
```

## Migration

1. Rename `[[epochs.phases.tasks]]` → `[[epochs.phases.goals]]` in plan.toml
2. Rename `[[plan.changes]]` → `[[goals]]` with nested `[[goals.tasks]]` in impl-plan
3. Update CLI commands: `exo plan add-task` → `exo goal add`
4. Update CLI commands: `exo impl add-step` → `exo task add`
5. Phase Details pane projects from both files

## Success Criteria

- [ ] Single mental model: goals contain tasks
- [ ] Phase Details shows goals with nested tasks
- [ ] `exo task list` and `exo phase status` show consistent data
- [ ] Walkthrough renders from goal completion logs
- [ ] No more "where does this live?" confusion
