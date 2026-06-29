<!-- exo:10174 ulid:01kmzxey1cbk8r4857zxk17ty8 -->


# RFC 10174: Inbox System (Hierarchical Intent Queue)

- **Supersedes**: RFC 0124 (Async Intent Channel), RFC 0116 (Feedback System), RFC 00185 (Inbox-Driven Sidebar Actions), RFC 10071 (Context Inbox), RFC 0016 (Attention is Sovereign)

| Field          | Value                                              |
| -------------- | -------------------------------------------------- |
| **ID**         | 10174                                              |
| **Title**      | Inbox System (Hierarchical Intent Queue)           |
| **Stage**      | 1 (Proposal)                                       |
| **Status**     | Active                                             |
| **Created**    | 2026-02-19                                         |
| **Supersedes** | RFC 0124, RFC 0116, RFC 00185, RFC 10071, RFC 0016 |

---

## Summary

This RFC defines the **Inbox System** — the unified communication channel between user and agent in Exosuit, with **hierarchical scheduling and disposition** semantics.

The core insights:

1. User intents should be **queued** (not lost) and **surfaced contextually** (not interrupting)
2. Intents should behave like **RAII resources** — attached to scopes, presented at checkpoints, and explicitly disposed before scope exit
3. The inbox is a **channel**, not a backdoor — all user actions flow through as intents

## The Axiom: Attention is Sovereign

> **Attention is Sovereign** — The user's focus is a sacred resource. The system must never forcibly redirect attention unless system integrity is critical. All non-critical context updates must be queued for voluntary ingestion.

This axiom shapes the entire inbox design:

- No interrupting toasts or popups
- Passive indicators (status bar badge, tree view)
- User-initiated review ("pull" not "push")
- Agent processes intents asynchronously

### The "Desk" Metaphor

We use a "Desk" metaphor rather than a "Pop-up" metaphor:

- **The Editor**: Your immediate workspace
- **The Inbox**: A physical tray on the corner of the desk
- **The Agent**: A colleague who places a document in the tray and walks away silently, trusting you to read it when ready

## Motivation

### The Problem

Users have thoughts, corrections, and guidance throughout a session. Without a proper channel:

1. **No persistence** — Intents captured in chat are lost on context compaction
2. **No contextual surfacing** — Even if captured, intents aren't shown when relevant
3. **Chat is ephemeral** — The primary communication channel evaporates
4. **Sidebar bypasses agent** — UI buttons directly mutate state, leaving the agent unaware
5. **Overwhelm** — All pending intents treated uniformly, presented at once
6. **Silent dropping** — Intents can be forgotten without explicit decision

### The Insight: Scopes as Scheduling Boundaries

The work hierarchy (task → goal → phase → epoch) provides natural presentation boundaries. An intent attached to a goal should be:

- **Presented** when tasks under that goal complete (reminder)
- **Required** to be disposed before the goal can complete (gate)

This mirrors JavaScript's task/microtask queue, but with nested scopes and mandatory acknowledgment.

## Proposal

### 1. Intent Schema

```toml
[[intent]]
id = "intent-01KG..."
created = "2026-02-19T10:30:00Z"
status = "pending"  # pending | acknowledged | resolved | archived

# Content
category = "correction"  # correction | guidance | question | priority
subject = "Variable naming convention"
body = """
Please use snake_case for all Rust identifiers, not camelCase.
I noticed some inconsistency in the last few commits.
"""

# Scope attachment — unified field for both targeting and scheduling
# If null, intent is "untriaged" and appears in Inbox section
attached_to = "goal:01KGABC..."  # task:<id> | goal:<id> | phase:<id> | epoch:<id> | inbox:<id>

# Content scope (what kind of work this relates to)
scope = "rust"  # global | phase:<id> | file:<path> | rust | typescript

# Urgency (when to surface)
urgency = "next-touch"  # immediate | next-touch | when-relevant

# Optional: structured action (for sidebar-initiated intents)
[intent.action]
type = "complete-goal"  # complete-goal | complete-task | verify-task | add-note | dismiss | escalate | convert-to-task
evidence = "All tasks verified, tests passing"
```

The `attached_to` field serves dual purposes:

- **Feedback targeting**: "This intent is _about_ this item" (shows badge on item)
- **Scheduling scope**: "This intent is _scoped to_ this item" (gates item completion)

### 2. Scopes Are Nested (RAII-style)

```
task ⊂ goal ⊂ phase ⊂ epoch ⊂ project
```

An intent attached to a scope is **in scope** for all containing scopes. Attaching to `goal:X` means the intent is relevant to that goal's phase and epoch.

### 3. Sub-scope Completion Triggers Presentation

When a **sub-scope completes**, all intents attached to the **containing scope** are presented:

| When this completes... | These intents are presented |
| ---------------------- | --------------------------- |
| Task                   | Goal-scoped intents         |
| Goal                   | Phase-scoped intents        |
| Phase                  | Epoch-scoped intents        |

This creates natural "nag" checkpoints without manual scheduling.

### 4. Scope Exit Requires Disposition

To **exit a scope**, all intents attached to that scope must be **disposed**. This is a hard gate — the agent cannot mark a goal complete while it has undisposed intents.

Disposition options:

| Disposition       | Meaning                                                         |
| ----------------- | --------------------------------------------------------------- |
| **Resolve**       | Triage action taken (promote to goal, dismiss, convert to task) |
| **Defer-up**      | Move to containing scope (goal→phase→epoch)                     |
| **Defer-lateral** | Move to a different item at same level (goal X → goal Y)        |
| **Defer-forward** | Move to a future scope (next phase, specific future goal)       |

### 5. Presentation ≠ Disposition

Being presented does not dispose an intent. The agent must take explicit action. If no action is taken, the intent remains and will be **re-presented** at the next sub-scope completion.

This prevents intents from being silently dropped.

### 6. Two-Phase Defer Behavior

Deferring an intent has two distinct moments:

1. **During scope execution**: The agent can acknowledge an intent without resolving it. The intent stays at its current scope and will be re-presented at the next checkpoint (sub-scope completion).

2. **At scope finalization**: When the agent attempts to exit the scope (complete a goal, finish a phase), all attached intents **must** have a disposition. The agent cannot simply "leave them behind" — they must either be resolved or explicitly moved to a new scope.

This means "I'll deal with it later" is valid _during_ execution, but at the exit gate, "later" must be specified: which scope will own this intent next?

```
During goal execution:
  [intent presented] → "Not now" → [stays at goal, re-presented next task completion]

At goal completion:
  [intent still attached] → BLOCKED
  Options: Resolve | Defer to phase | Defer to different goal | Defer to future phase
```

### 7. Steering Integration

The steering response includes intents grouped by relevance:

```rust
pub struct SteeringBlock {
    pub progress: ProgressHeuristic,
    pub active_work: Option<ActiveWork>,
    pub suggestions: Vec<Suggestion>,
    pub pending_intents: PendingIntents,  // Grouped by disposition requirement
}

pub struct PendingIntents {
    pub gate: Vec<SurfacedIntent>,      // Must dispose before current scope exits
    pub reminder: Vec<SurfacedIntent>,  // Attached to containing scope, presented as checkpoint
    pub untriaged: Vec<SurfacedIntent>, // No attachment, needs triage
}

pub struct SurfacedIntent {
    pub id: String,
    pub category: IntentCategory,
    pub subject: String,
    pub attached_to: Option<CanonicalRef>,
    pub relevance: f32,  // 0.0 - 1.0, computed from context match
}
```

Surfacing logic:

- `immediate` → always include in `gate`
- `next-touch` → include on next phase/task transition
- `when-relevant` → include when scope matches current work

### 8. Sidebar Actions as Intents

Sidebar buttons should add intents to `inbox.toml` instead of directly calling CLI commands:

```typescript
// WRONG: Direct mutation (agent is blind)
await exec(`exo goal complete "${goalId}"`);

// RIGHT: Route through inbox (agent is aware)
await addInboxIntent({
  category: "priority",
  subject: "Complete goal: Implement sidebar routing",
  attached_to: `goal:${goalId}`,
  urgency: "immediate",
  action: { type: "complete-goal", evidence: "All tasks verified" },
});
```

#### Action Types

| Action Type       | Fields     | Description                             |
| ----------------- | ---------- | --------------------------------------- |
| `complete-goal`   | `evidence` | Mark goal as complete                   |
| `complete-task`   | `evidence` | Mark task as complete                   |
| `verify-task`     | `evidence` | Verify task with evidence               |
| `add-note`        | `note`     | Add note to item                        |
| `dismiss`         | `reason`   | User no longer needs this addressed     |
| `escalate`        | -          | Bump urgency so agent sees it sooner    |
| `convert-to-task` | `goal`     | Convert inbox item to a task under goal |

### 9. Resolution Flow

```
User adds intent (UI/chat) → inbox.toml
     ↓
Agent reads steering → sees pending_intents (grouped)
     ↓
Agent acknowledges → status = "acknowledged"
     ↓
Agent acts on intent
     ↓
Agent resolves → status = "resolved" + resolution_note
     ↓
Periodic archive → moves old resolved items to inbox-archive.toml
```

### 10. Intent Display States

Intents have three distinct display states based on their `attached_to` and `urgency`:

| State                | Condition                               | Visual Treatment                                 |
| -------------------- | --------------------------------------- | ------------------------------------------------ |
| **Untriaged**        | `attached_to` is null                   | Shown in Inbox section                           |
| **Attached**         | `attached_to` points to goal/task/phase | Badge on the referenced item, NOT in Inbox       |
| **Steering-pending** | `urgency: immediate`                    | Hidden or subtle indicator (agent will see soon) |

## Examples

### Example 1: Idea During Task Execution

User has an idea while working on a task:

```
User: "We should add retry logic here"
→ Intent created, attached_to: goal:current
```

- Intent is presented when the current task completes
- Agent can: resolve it (add as task), defer to phase, or defer to a future goal
- If not disposed, it gates goal completion

### Example 2: Bug Report for Future Phase

User reports a bug that's out of scope:

```
User: "Found a bug in the auth flow"
→ Intent created, attached_to: phase:next-phase
```

- Intent is NOT presented during current phase
- When next-phase is entered, intent becomes active
- Presented when goals in that phase complete

### Example 3: Papercut for Later

User notes a minor issue:

```
User: "The error message here is confusing"
→ Intent created, attached_to: epoch:current
```

- Presented when phases complete
- Low urgency, but won't be forgotten
- Must be disposed before epoch ends

## Compatibility

- **Supersedes RFC 0124** — This RFC is the comprehensive inbox system specification
- **Backward compatible** — Intents without `attached_to` are treated as untriaged
- **Incremental adoption** — Can implement presentation logic before gating logic
- **Migration**: `subject_ref` field from 0124 becomes `attached_to`

## Implementation Plan

### Phase 1: Schema Extension (Partially Complete ✅)

- [x] Intent schema with full lifecycle tracking
- [x] CLI Commands: `exo inbox list/add/ack/resolve/archive`
- [x] Archive system: `exo gc inbox [--days N]`
- [ ] Add `attached_to` field to Intent struct (extends existing `subject_ref`)
- [ ] Update `exo inbox add` to accept `--attached-to` flag

### Phase 2: Presentation Logic

- [ ] Modify steering to group intents by scope relevance (gate/reminder/untriaged)
- [ ] Present goal-scoped intents when tasks complete
- [ ] Present phase-scoped intents when goals complete

### Phase 3: Disposition Gating

- [ ] Add disposition check to `exo goal complete`
- [ ] Add disposition check to `exo phase finish`
- [ ] Implement defer commands (`exo inbox defer <id> --to <scope>`)

### Phase 4: UI Integration

- [ ] Command: `exosuit.captureIntent` with keybinding (`Cmd+Shift+I`)
- [ ] Service: InboxStatusBarService (watcher + badge)
- [ ] Command: `exosuit.openInboxQuickPick`
- [ ] LM Tool: `exo-inbox` for agent access
- [ ] Show attached intents as badges on their target items
- [ ] Show disposition prompt when completing scoped work
- [ ] Update Inbox section to only show untriaged intents

---

## UI Integration (VS Code Extension)

### User-Facing Commands

| Command                      | Keybinding    | Description                        |
| ---------------------------- | ------------- | ---------------------------------- |
| `exosuit.captureIntent`      | `Cmd+Shift+I` | Quick capture intent modal         |
| `exosuit.openInboxQuickPick` | -             | Show pending intents in Quick Pick |
| `exosuit.focusInbox`         | -             | Open inbox tree view / webview     |
| `exosuit.resolveIntent`      | -             | Mark intent as resolved            |

### Status Bar Indicator

- **Hidden**: When count = 0
- **Badge**: `$(inbox) 3` when count > 0
- **Tooltip**: "3 pending intents - click to review"
- **Click behavior**: Opens Quick Pick

### Quick Capture Flow

1. User presses `Cmd+Shift+I` while editing code
2. Extension detects current file → scope, selected text → pre-fill subject
3. Modal sequence: Subject, Category, Urgency, Body (optional)
4. Extension calls `exo inbox add` via CLI
5. Status bar updates immediately

### Notification Strategy

Following "Attention is Sovereign":

- **No interrupting toasts** for new intents
- **Passive indicator**: Status bar badge
- **User-initiated review**: Click to triage

---

## Machine Channel Wire Format

### Add Intent

```json
{
  "op": "intent",
  "action": "add",
  "category": "correction",
  "subject": "Use snake_case",
  "body": "Rust identifiers should use snake_case...",
  "attached_to": "goal:01KGABC...",
  "scope": "rust",
  "urgency": "next-touch"
}
```

### Resolve Intent

```json
{
  "op": "intent",
  "action": "resolve",
  "id": "intent-01KE...",
  "resolution": "Applied snake_case in commit abc123"
}
```

### Defer Intent

```json
{
  "op": "intent",
  "action": "defer",
  "id": "intent-01KE...",
  "to": "phase:01KGXYZ..."
}
```

---

## References

- RFC 00224: The SOAR Loop — workflow model that defines scope boundaries
- RFC 00242: Progress Tool — mid-task steering touchpoints (should reference this RFC's queueing model)
- RFC 10176: Project State Model — defines inbox schema (this RFC defines behavioral semantics)
- RFC 10165: Reactive SQLite — storage implementation

**Epoch:** SQLite as Source of Truth (Phase 1 complete)
