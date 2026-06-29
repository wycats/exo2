<!-- exo:10182 ulid:01kn7krx033jtx42gr6qn06r8r -->

# RFC 10182: Contextual Steering: Every Command is a Perception Touchpoint

## Summary

Every exo command that operates on an entity (task, goal, phase) should return entity-scoped steering that includes perception data from the inbox. Today, the agent only discovers user feedback when it explicitly calls `exo status`. During execution — the mode where it spends most of its time — the agent is blind to user feedback, concerns, and completion claims.

This RFC designs the interface and implementation for making every command a perception touchpoint, so that user feedback reaches the agent organically at the moments when it can act on it.

## Motivation

### The principle (RFC 10170)

> "Tools should be designed so that using them naturally produces steering as a side effect."

Task and goal commands are the mutation boundaries where the agent pauses between units of work. They're the natural place to inject perception. But today, `task start`, `task log`, `task complete`, and `goal complete` all return hardcoded steering — static suggestions like "List tasks" and "Show map" — with zero perception data.

### The problem in practice

A user clicks "Add Feedback" on a goal in the sidebar: *"I don't think we need OAuth here — simple tokens might be enough."* This lands in the inbox with `entity_type: goal`, `entity_id: auth-system`, `intent: concern`.

The agent, working on a task under that goal, calls `exo task log auth-task --log "Implemented OAuth flow"`. The response is:

```
Task 'auth-task' logged.
→ Next: exo task list (check remaining tasks)
```

The agent has no idea the user just pushed back on the entire approach. It continues implementing OAuth. The user's feedback sits in the inbox, invisible until someone calls `exo status`.

### The fix

The same `task log` call should return:

```
Task 'auth-task' logged.

⚠ User concern on goal 'auth-system' (12 min ago):
  "I don't think we need OAuth here — simple tokens might be enough"
  → exo inbox list --entity-type goal --entity-id auth-system

→ Next: exo task list (check remaining tasks)
```

The agent didn't ask for feedback. It reported progress. The system told it what it needed to know.

## Design

### Three levels of steering resolution

| Level | Context source | Available today | Impact |
|-------|---------------|-----------------|--------|
| **Project-level** | `exo status` / `exo map` | ✅ Yes | Good for orientation, useless during execution |
| **Command-level** | Entity ID from the command itself | Infrastructure exists, not wired | High — closes the primary gap |
| **Activity-level** | Copilot hooks, file edits, conversation | Not yet | Aspirational — would surface axiom violations, pattern drift |

This RFC focuses on **Level 2: command-level contextual steering.** Level 3 is a future extension that uses the same interface.

### Entity-scoped perception

When a command operates on an entity, the steering response scopes perception to the **entity tree**: task → parent goal → phase. This means:

- `task log my-task` returns feedback on `my-task` AND on its parent goal
- `goal complete auth-system` returns feedback on `auth-system` AND on its tasks
- `phase finish` returns feedback on the phase AND all its goals

The scoping uses the entity relationships already in SQLite (tasks belong to goals via `goal_id`, goals belong to phases).

### The steering response interface

Commands that return steering already use `SteeringBlock`. The change is to populate the existing `perception_summaries` field for entity-scoped commands instead of leaving it empty.

```rust
pub struct SteeringBlock {
    // ... existing fields ...
    pub perception_summaries: Vec<PerceptionSummary>,
    // NEW: the entity context that scoped this steering
    pub entity_context: Option<EntityContext>,
}

pub struct EntityContext {
    pub entity_type: String,
    pub entity_id: String,
    /// Parent entities in the hierarchy (task → goal → phase)
    pub ancestors: Vec<(String, String)>,
}
```

`PerceptionSummary` already has the right shape:

```rust
pub struct PerceptionSummary {
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub count: usize,
    pub highest_priority: InboxPriority,
    pub sample_subject: String,
    pub drill_in: String,
}
```

### Which commands get contextual steering

| Command | Entity context | Perception scope |
|---------|---------------|-----------------|
| `task start <id>` | task → goal → phase | Feedback on task + parent goal |
| `task log <id>` | task → goal → phase | Feedback on task + parent goal |
| `task complete <id>` | task → goal → phase | Feedback on task + parent goal |
| `goal complete <id>` | goal → phase | Feedback on goal + child tasks |
| `goal abandon <id>` | goal → phase | Feedback on goal |
| `phase finish` | phase | Feedback on phase + all goals |

Commands without entity context (`inbox add`, `rfc list`, `help`, etc.) continue with project-level or no steering.

### Implementation approach

#### Step 1: Shared steering function for entity-scoped commands

Create `derive_entity_steering(root, entity_type, entity_id, agent_id)` that:

1. Resolves the entity tree (task → goal → phase)
2. Queries `has_completion_claim()` for the entity
3. Queries inbox for `immediate` and `next-touch` items scoped to the entity tree
4. Runs `summarize_surfaced_intents()` on the filtered set
5. Builds a `SteeringBlock` with both entity-scoped perception and standard next actions

This is lighter than `derive_world_steering()` (no git status, no RFC pipeline, no epoch state) — just entity perception + contextual next actions.

#### Step 2: Wire into task/goal commands

Replace `default_task_steering()` calls in `task.rs` and `goal.rs` with `derive_entity_steering()`. The entity ID is already available in each command struct (`self.id`).

#### Step 3: Fix relevance scoring for entity types

In `inbox.rs`, the relevance scorer has a TODO for goal/task/RFC matching. Complete it so that `when-relevant` items properly surface when the agent is working on the relevant entity.

#### Step 4: Fix TypeScript type alignment

The TS `SteeringBlock` in `progress.ts` uses `pending_intents: SurfacedIntent[]` (stale). Update to `perception_summaries: PerceptionSummary[]` to match the Rust struct.

### Future: Activity-level steering (Level 3)

Copilot hooks and tool-call observation would provide *implicit* entity context — inferring which goal the agent is working on from files edited, conversation content, and tool patterns. This enriches the same `EntityContext` interface:

- **Explicit context** (Level 2): The command's entity ID
- **Inferred context** (Level 3): The hook observer's best guess at the active entity

Both feed into the same `derive_entity_steering()` function. Level 3 is additive — it doesn't change the interface, just provides richer input.

This is also where axiom enforcement would happen: the system knows which files are being edited (from hooks), can check whether the changes align with documented axioms, and surface violations as steering signals. But that's a separate RFC.

## Relationship to existing RFCs

- **RFC 10181** (Shared Perception): Defines the inbox as a steering channel. This RFC designs how that channel reaches the agent during execution, not just at orientation points.
- **RFC 00242** (Progress Tool): Describes `task log` with intermediate steering. This RFC generalizes that design to all entity commands and specifies the perception interface.
- **RFC 10170** (Mutation Boundaries): States the principle. This RFC implements it.
- **RFC 00224** (SOAR Loop): The Status phase gets project-level steering; this RFC adds perception to the Act phase.

## Resolved questions

1. **Performance**: `derive_entity_steering()` bypasses `AgentContext::load` (no git status, no RFC pipeline, no epoch state) and goes straight to `SqliteLoader` for inbox queries + entity tree resolution. With WAL mode the query is a single indexed lookup — microseconds. Acceptable.

2. **Noise threshold**: 1 perception summary per entity in the tree, max 3 total. Task feedback + parent goal feedback + phase feedback. Multiple inbox items on the same entity are collapsed by `summarize_surfaced_intents()` (already implemented — groups by entity, shows count).

3. **Recency weighting**: Yes. Include relative timestamp after the sample subject — "(12 min ago)" or "(2 days ago)". The `created_at` field is already available; this is formatting only.

4. **Completed entity feedback**: A concern on a completed goal surfaces as a **repair action**, not just a perception summary. This gives it urgency: "⚠ User concern on completed goal 'X' — may need to reopen." Repair actions are already a distinct field in `SteeringBlock`.

5. **Activity-level hook design**: Addressed by RFC 10183 (Agent Activity Model). Level 3 steering consumes `ActivityContext` projections from the event log. The hook infrastructure (daemon-side command capture + extension file-save watcher) is designed there.
