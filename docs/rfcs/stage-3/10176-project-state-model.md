<!-- exo:10176 ulid:01kmzx900c67rjbtsb225y1y18 -->


# RFC 10176: Project State Model

- **Supersedes**: RFC 00177, RFC 00229, RFC 0022, RFC 10161, RFC 10102

## Summary

Storage-agnostic data model for Exosuit project state. Defines entities, relationships, authority semantics, and state machines independent of persistence format (TOML, SQLite, etc.).

This RFC consolidates concepts from RFCs 00177 (Goals/Tasks), 00229 (Goal Status Authority), and 10161 (Plan Object Model) into a single conceptual foundation. It also defines secondary entities (ideas, inbox) and the tagged union normalization pattern for SQLite storage.

## Motivation

Multiple RFCs describe overlapping aspects of the project state model:

- **RFC 00177** defines the goal/task hierarchy and FK pattern
- **RFC 00229** defines authority semantics (status vs derived signals)
- **RFC 10161** describes the TOML implementation
- **RFC 10028** defines the phase state machine

This fragmentation obscures the actual data model. As we migrate to SQLite (RFC 10165), we need a single conceptual foundation that:

1. Abstracts storage details (tables/views instead of TOML files)
2. Preserves authority semantics
3. Unifies the entity hierarchy
4. Serves as the specification that implementations (TOML, SQLite) realize

## Entity Hierarchy

### Tables

```
┌─────────────────────────────────────────────────────────────────┐
│  epochs                                                         │
│    id: ULID (PK)                                               │
│    title: string                                                │
│    slug: string                                                 │
│    reviewed: bool                                               │
├─────────────────────────────────────────────────────────────────┤
│  phases                                                         │
│    id: ULID (PK)                                               │
│    title: string                                                │
│    status: pending | in-progress | completed                    │
│    kind: normal | chore                                         │
│    epoch_id: FK → epochs                                        │
│    sort_key: TEXT (fractional index, lexicographic ordering)    │
├─────────────────────────────────────────────────────────────────┤
│  goals                                                          │
│    id: ULID (PK)                                               │
│    label: string                                                │
│    status: pending | in-progress | completed | abandoned        │
│    phase_id: FK → phases                                        │
│    rfc: string? (RFC link)                                      │
│    completion_log: string? (required for completed/abandoned)   │
│    sort_key: TEXT (fractional index, lexicographic ordering)    │
├─────────────────────────────────────────────────────────────────┤
│  tasks                                                          │
│    id: ULID (PK)                                               │
│    title: string                                                │
│    status: pending | in-progress | completed | skipped          │
│    goal_id: FK → goals                                          │
│    completion_log: string?                                      │
│    sort_key: TEXT (fractional index, lexicographic ordering)    │
└─────────────────────────────────────────────────────────────────┘
```

### Relationships

```
epoch ──1:N──▶ phase ──1:N──▶ goal ──1:N──▶ task
```

- Each epoch contains multiple phases
- Each phase contains multiple goals
- Each goal contains multiple tasks
- Tasks reference goals by FK only (no duplicated metadata)

### Views

- **active_phase**: Single row view of the currently executing phase
- **phase_execution**: Tasks for active phase joined with goal metadata

## Authority Model

### Metadata vs Execution

| Category      | Entities              | Lifecycle                          | Authority                |
| ------------- | --------------------- | ---------------------------------- | ------------------------ |
| **Metadata**  | epochs, phases, goals | Persistent across phases           | Authoritative for status |
| **Execution** | tasks                 | Phase-scoped, reset on phase start | Derived signal only      |

### Status Authority

**Key Invariant**: Goal status is authoritative. Task completion is a derived signal.

```
goal.status = 'completed' ⟺ (all_tasks_done ∧ completion_log.is_some())
```

- The `goals` table is the single source of truth for goal status
- Task completion informs steering but does not determine goal status
- A goal cannot be marked complete without a completion log

### Derived Signals

These are computed, not stored:

| Signal            | Type | Purpose                       |
| ----------------- | ---- | ----------------------------- |
| `tasks_complete`  | bool | All tasks for a goal are done |
| `pending_count`   | int  | Tasks not yet started         |
| `completed_count` | int  | Tasks finished                |

Derived signals are used by **steering** (suggesting next actions) but not for **status determination**.

### Anti-Pattern: Duplicating Metadata

Tasks should reference goals by FK only. Do not duplicate goal metadata (label, RFC link, status) into the tasks table.

```
# WRONG: Duplicated metadata
tasks: [{ goal_label: "Implement X", goal_rfc: "10165", ... }]

# RIGHT: FK reference only
tasks: [{ goal_id: "01ABC...", ... }]
```

### Pattern: Tagged Union Normalization

When a Rust enum with associated data (tagged union) needs to be stored in SQLite, normalize it into separate columns:

```
// Rust type
enum IntentScope {
    Global,
    Phase(String),
    File(String),
    Rust,
    Typescript,
}

// SQLite columns
scope_type: TEXT NOT NULL CHECK (scope_type IN ('global', 'phase', 'file', 'rust', 'typescript'))
scope_value: TEXT  -- NULL for variants without associated data
```

**Rules**:

1. `_type` column holds the discriminant (enum variant name)
2. `_value` column holds the associated data (NULL if none)
3. Use CHECK constraints to enforce valid discriminants
4. Use CHECK constraints to enforce value presence/absence per variant

**Example CHECK constraint**:

```sql
CHECK (
    (scope_type IN ('global', 'rust', 'typescript') AND scope_value IS NULL) OR
    (scope_type IN ('phase', 'file') AND scope_value IS NOT NULL)
)
```

**Rationale**: This pattern enables:

- SQL queries that filter by variant type
- Indexing on the type column
- Schema evolution (adding variants) via migrations
- Type-safe reconstruction in Rust

**Anti-pattern**: Storing tagged unions as JSON blobs. This prevents SQL-level filtering and indexing.

## Goal State Machine

```
         ┌──────────────────────────────────────────┐
         │                                          │
         ▼                                          │
    ┌─────────┐     ┌─────────────┐     ┌───────────┴───┐
    │ pending │────▶│ in-progress │────▶│   completed   │
    └─────────┘     └─────────────┘     │ (log required)│
                           │            └───────────────┘
                           │
                           ▼
                    ┌─────────────┐
                    │  abandoned  │
                    │(log required)│
                    └─────────────┘
```

- **pending**: Goal exists but work hasn't started
- **in-progress**: Active work on this goal
- **completed**: All tasks done, completion log recorded
- **abandoned**: Goal dropped, abandonment log recorded (tasks need not be done)

## Secondary Entities

Beyond the core execution hierarchy (epochs → phases → goals → tasks), the project state model includes secondary entities that support planning and communication.

### Ideas

Ideas capture feature suggestions, improvements, and observations that may inform future work. They exist outside the execution hierarchy.

```
┌─────────────────────────────────────────────────────────────────┐
│  ideas                                                          │
│    id: ULID (PK)                                               │
│    text_id: string (UNIQUE)     -- UUID from TOML migration    │
│    title: string                                                │
│    description: string?                                         │
│    status: new | archived                                       │
│    created_at: datetime                                         │
│    source: user | agent                                         │
├─────────────────────────────────────────────────────────────────┤
│  idea_tags (junction)                                           │
│    idea_id: FK → ideas                                         │
│    tag: string                                                  │
│    PRIMARY KEY (idea_id, tag)                                   │
├─────────────────────────────────────────────────────────────────┤
│  idea_task_refs (junction)                                      │
│    idea_id: FK → ideas                                         │
│    task_ref: string             -- task ID reference            │
│    PRIMARY KEY (idea_id, task_ref)                              │
└─────────────────────────────────────────────────────────────────┘
```

**Relationships**:

- Ideas are independent of the epoch/phase hierarchy
- Ideas may reference tasks via `idea_task_refs` (many-to-many)
- Ideas may have multiple tags via `idea_tags` (many-to-many)

**Authority**: Ideas have no derived status — `status` is authoritative.

### Inbox

The inbox provides a persistent queue for user→agent intent communication. Intents survive context compaction and are surfaced when contextually relevant.

```
┌─────────────────────────────────────────────────────────────────┐
│  inbox                                                          │
│    id: ULID (PK)                                               │
│    text_id: string (UNIQUE)     -- UUID from TOML migration    │
│    created: datetime                                            │
│    updated: datetime?                                           │
│    status: pending | acknowledged | resolved | archived         │
│    category: correction | guidance | question | priority        │
│    urgency: immediate | next-touch | when-relevant              │
│    subject: string              -- brief summary                │
│    body: string                 -- full content                 │
│    resolution: string?          -- when status = resolved       │
│                                                                 │
│    -- Normalized tagged unions (see pattern below)              │
│    scope_type: global | phase | file | rust | typescript        │
│    scope_value: string?         -- NULL for global/rust/ts      │
│                                                                 │
│    subject_ref_type: goal | task | phase | rfc | NULL           │
│    subject_ref_id: string?      -- NULL when no subject_ref     │
│                                                                 │
│    action_type: complete-goal | complete-task | verify-task |   │
│                 add-note | NULL                                  │
│    action_payload: string?      -- evidence or note content     │
└─────────────────────────────────────────────────────────────────┘
```

**Scope Types**:
| Type | Value | Example |
|------------|----------------------|--------------------------------|
| global | NULL | Always relevant |
| phase | phase ID | Relevant to specific phase |
| file | file path | Relevant to specific file |
| rust | NULL | Relevant to any .rs file |
| typescript | NULL | Relevant to any .ts/.tsx file |

**Subject Reference Types**:
| Type | ID Format | Example |
|-------|----------------------|--------------------------------|
| goal | goal text ID | `goal:schema-extension` |
| task | task ULID | `task:01KGABC123` |
| phase | phase ULID | `phase:01KGABC123` |
| rfc | RFC number | `rfc:00185` |

**Action Types**:
| Type | Payload | Purpose |
|---------------|----------------------|--------------------------------|
| complete-goal | evidence (optional) | Mark goal as complete |
| complete-task | evidence (optional) | Mark task as complete |
| verify-task | evidence (optional) | Verify a task |
| add-note | note content | Add a note to an item |

**Authority**: Inbox status is authoritative. The agent transitions items through the lifecycle.

**Lifecycle**:

```
pending → acknowledged → resolved → archived
```

## Phase State Machine

See RFC 10028 for the full phase state machine specification. Key states:

1. **NoActivePhase**: No phase is currently executing
2. **ActivePhase:Unprepared**: Phase started but no execution plan
3. **ActivePhase:Executing**: Phase has tasks, work in progress
4. **ActivePhase:ReadyToFinish**: All gates satisfied
5. **PreparingNextPhase**: Between phases, preparing next
6. **PreparingNextEpoch**: Between epochs

### Overlays

- **SurgicalStrike**: Single-depth tangent mode for urgent work
- **NeedsUpgrade**: Hard gate when deprecated artifacts exist

## Ordering: Fractional Indexing

Ordered entities (phases, goals, tasks) use a `sort_key TEXT` column for position.
Keys are generated by the `fractional_index` crate (hex-encoded, lexicographically sortable).

- **Append**: `FractionalIndex::new_after(last_key)`
- **Prepend**: `FractionalIndex::new_before(first_key)`
- **Insert between**: `FractionalIndex::new_between(before_key, after_key)`
- **Query**: `ORDER BY sort_key NULLS LAST, id`

This enables O(1) reorder operations without renumbering siblings.
See RFC 10032 (Position Protocol) for the full API design.

During TOML→SQLite import, sort_keys are generated from array position.
The skip path backfills NULL sort_keys to prevent mixed NULL/non-NULL ordering.

## Transactional Mutations

All state changes go through a mutation layer that:

1. Validates the change against invariants
2. Applies the change atomically
3. Persists to storage
4. Notifies observers (reactivity)

This pattern ensures the persisted state is always consistent with the in-memory model.

## Implementation Notes

- **RFC 10161** described the TOML implementation (`plan.toml` structure) — now superseded
- **RFC 10165** describes the SQLite implementation (virtual tables, reactive queries)
- This RFC is the conceptual layer that implementations realize

**Epoch:** SQLite as Source of Truth (Phase 1 complete)

### Loading Strategy: Eager vs Lazy

The execution hierarchy (epochs → phases → goals → tasks) is loaded eagerly into `ExoState` via `load_state()`. Secondary entities (ideas, inbox) are loaded lazily via separate methods (`load_ideas()`, `load_inbox()`).

**Rationale**: Ideas and inbox are not always needed. Steering may need them; simple status queries don't. Lazy loading avoids unnecessary work.

**Tradeoff**: If we later want a unified "project state changed" reactive signal, we'll need to either:

1. Add ideas/inbox to `ExoState` and load eagerly, or
2. Implement separate change tracking for secondary entities

This is an open design question. The current implementation matches the TOML-era behavior (separate files, separate loads).

## Supersedes

- **RFC 00177** (Goals and Tasks: Unified Work Item Model) — hierarchy and FK pattern
- **RFC 00229** (Goal Status Authority) — authority semantics
- **RFC 0022** (Unified Project State) — old TypeScript `ContextService` project-state model
- **RFC 10161** (The Plan Object Model) — TOML implementation details

## References

- **RFC 10028** (Phase State Machine & Projections) — phase lifecycle (not superseded, complementary)
- **RFC 10165** (Reactive SQLite) — SQLite implementation target
- **RFC 10174** (Hierarchical Intent Queue) — inbox behavioral semantics (scope gating, disposition, steering integration)
