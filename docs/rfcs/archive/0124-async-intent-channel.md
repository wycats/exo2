<!-- exo:124 ulid:01kg5kp2h6eat65eqhqs9hjryq -->

# RFC 124: Inbox System: Async Intent Channel


# RFC 0124: Inbox System (Async Intent Channel)

- **Supersedes**: RFC 0116 (Feedback System), RFC 00185 (Inbox-Driven Sidebar Actions), RFC 10071 (Context Inbox)
- **Absorbs**: "Attention is Sovereign" axiom from RFC 0016/10071

| Field          | Value                                    |
| -------------- | ---------------------------------------- |
| **ID**         | 0124                                     |
| **Title**      | Inbox System (Async Intent Channel)      |
| **Stage**      | 3 (Candidate)                            |
| **Status**     | Active                                   |
| **Created**    | 2025-01-22                               |
| **Updated**    | 2026-02-19                               |
| **Author**     | Exosuit Team                             |
| **Supersedes** | RFC 0116, RFC 00185, RFC 10071, RFC 0016 |

---

## Summary

This RFC defines the **Inbox System** — the unified communication channel between user and agent in Exosuit. The core insight: user intents should be **queued** (not lost) and **surfaced contextually** (not interrupting).

The inbox is a **channel**, not a backdoor to mutate state. All user actions — whether capturing feedback, completing goals, or dismissing inbox items — flow through the inbox as intents that the agent processes.

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

### The Insight

The sidebar should be a **channel to the agent**, not a backdoor to mutate state. User actions are **intents** that the agent should process, potentially with additional context or follow-up. This applies to:

- Capturing feedback/corrections
- Completing goals or tasks
- Adding notes to items
- **Acting on inbox items themselves** (dismiss, escalate, convert to task)

## Proposal

### 1. Rename: feedback.toml → inbox.toml

The current name implies passive collection. "Inbox" better captures:

- User-initiated messages awaiting processing
- Queue semantics (items are resolved/archived)
- Familiar mental model (email inbox)

### 2. Intent Categories

```toml
[[intent]]
id = "intent-001"
created = "2025-01-22T10:30:00Z"
status = "pending"  # pending | acknowledged | resolved | archived
category = "correction"  # correction | guidance | question | priority

# The actual content
subject = "Variable naming convention"
body = """
Please use snake_case for all Rust identifiers, not camelCase.
I noticed some inconsistency in the last few commits.
"""

# Item-level targeting (optional) — uses CanonicalRef syntax
# When present, UI can show "feedback waiting" indicator on the referenced item
subject_ref = "goal:01KGABC123..."  # goal:<ulid> | task:<ulid> | phase:<ulid> | rfc:<id>

# Context for surfacing
scope = "rust"  # global | phase:<id> | file:<path> | rust | typescript
urgency = "next-touch"  # immediate | next-touch | when-relevant
```

> **Amendment (2026-02-01):** Added `subject_ref` field using `CanonicalRef` syntax for item-level targeting. This supersedes RFC 0116's `target_id` concept, unifying feedback into the inbox system. When `subject_ref` is present, the UI should show a "feedback waiting" indicator on the referenced item.

### 3. Steering Integration

The steering system already computes WorldState → SteeringBlock. We add intent surfacing:

```rust
// In steering.rs
pub struct SteeringBlock {
    pub progress: ProgressHeuristic,
    pub active_work: Option<ActiveWork>,
    pub suggestions: Vec<Suggestion>,
    pub pending_intents: Vec<SurfacedIntent>,  // NEW
}

pub struct SurfacedIntent {
    pub id: String,
    pub category: IntentCategory,
    pub subject: String,
    pub subject_ref: Option<CanonicalRef>,  // Item-level targeting (amendment 2026-02-01)
    pub relevance: f32,  // 0.0 - 1.0, computed from context match
}
```

Surfacing logic:

- immediate → always include
- next-touch → include on next phase/task transition
- when-relevant → include when scope matches current work

### 4. Resolution Flow

```
User adds intent (UI/chat) → inbox.toml
     ↓
Agent reads steering → sees pending_intents
     ↓
Agent acknowledges → status = "acknowledged"
     ↓
Agent acts on intent
     ↓
Agent resolves → status = "resolved" + resolution_note
     ↓
Periodic archive → moves old resolved items to inbox-archive.toml
```

### 5. Intent Display States

Not all pending intents belong in the "Inbox" section. Intents have three distinct display states based on their `subject_ref` and `urgency`:

| State                | Condition                               | Visual Treatment                                 |
| -------------------- | --------------------------------------- | ------------------------------------------------ |
| **Untriaged**        | `subject_ref` is null                   | Shown in Inbox section                           |
| **Attached**         | `subject_ref` points to goal/task/phase | Badge on the referenced item, NOT in Inbox       |
| **Steering-pending** | `urgency: immediate`                    | Hidden or subtle indicator (agent will see soon) |

**Untriaged items** are free-floating — the user captured something but hasn't connected it to specific work. These appear in the Inbox section for triage.

**Attached items** have a `subject_ref` like `goal:01KG...` or `task:01KG...`. These represent feedback _about_ that item and should appear as a badge or decoration on the item itself, not in a separate inbox. Example: user clicks "Add Note" on a task → intent with `subject_ref: task:01KG...` → task shows 💬 badge.

**Steering-pending items** with `urgency: immediate` are already queued for the agent's next steering read. They're not "untriaged" — they're "in flight". These could be hidden entirely or shown with a distinct treatment (e.g., "⏳ Pending steering").

This distinction prevents the Inbox from becoming a dumping ground for items that have already been routed.

### 6. Sidebar Actions as Intents

Sidebar buttons should add intents to `inbox.toml` instead of directly calling CLI commands:

```typescript
// WRONG: Direct mutation (agent is blind)
await exec(`exo goal complete "${goalId}"`);

// RIGHT: Route through inbox (agent is aware)
await addInboxIntent({
  category: "priority",
  subject: "Complete goal: Implement sidebar routing",
  subject_ref: `goal:${goalId}`,
  urgency: "immediate",
  action: { type: "complete-goal", evidence: "All tasks verified" },
});
```

#### Action Types

The `[intent.action]` field supports structured actions:

| Action Type     | Fields     | Description               |
| --------------- | ---------- | ------------------------- |
| `complete-goal` | `evidence` | Mark goal as complete     |
| `complete-task` | `evidence` | Mark task as complete     |
| `verify-task`   | `evidence` | Verify task with evidence |
| `add-note`      | `note`     | Add note to item          |

#### Agent Processing

When the agent sees an action intent:

1. **Acknowledge** — Mark intent as acknowledged
2. **Validate** — Check if action is appropriate (e.g., are all tasks done?)
3. **Execute** — Call the underlying CLI command
4. **Resolve** — Mark intent as resolved with outcome

This allows the agent to ask clarifying questions, suggest additional actions, or maintain awareness of user activity.

### 7. Actions on Inbox Items

User actions on inbox items themselves also create intents. This ensures the agent is aware of user decisions and can respond appropriately.

#### The Pattern

When a user acts on an inbox item (dismiss, escalate, convert to task), the UI creates a _new_ intent targeting the original item:

```toml
[[intent]]
id = "intent-02KG..."
subject = "User dismissed: Review PR comments"
subject_ref = "inbox:intent-01KG..."  # Points to the original inbox item
urgency = "immediate"

[intent.action]
type = "dismiss"
reason = "Already handled in standup"
```

#### Inbox-Item Action Types

| Action Type       | Fields   | Description                                    |
| ----------------- | -------- | ---------------------------------------------- |
| `dismiss`         | `reason` | User no longer needs this addressed            |
| `escalate`        | -        | Bump urgency so agent sees it sooner           |
| `convert-to-task` | `goal`   | User wants this converted to a task under goal |

#### Why Not Direct Mutation?

Direct mutation (e.g., `exo inbox resolve <id>`) breaks the communication contract:

1. **Agent blindness** — Agent may have been planning to act on the item
2. **Lost context** — User's reason for dismissing isn't captured
3. **No feedback loop** — Agent can't ask "wait, I was about to do that"

By routing through the inbox, the agent sees "user dismissed X" in steering and can respond appropriately.

### 8. UI Feedback Badges

Items with pending intents show a "feedback waiting" indicator:

```typescript
function getItemDecorations(
  itemId: string,
  pendingIntents: SurfacedIntent[],
): vscode.ThemeIcon {
  const hasPendingIntent = pendingIntents.some(
    (intent) =>
      intent.subject_ref === `goal:${itemId}` ||
      intent.subject_ref === `task:${itemId}`,
  );

  if (hasPendingIntent) {
    return new vscode.ThemeIcon("comment-unresolved");
  }
  return getDefaultIcon(itemId);
}
```

### 9. Machine Channel Protocol

Extend existing feedback operation:

```json
{
  "op": "intent",
  "action": "add",
  "data": {
    "category": "correction",
    "subject": "Variable naming",
    "body": "Use snake_case...",
    "scope": "rust",
    "urgency": "next-touch"
  }
}

{
  "op": "intent",
  "action": "resolve",
  "data": {
    "id": "intent-001",
    "resolution": "Applied snake_case convention in commit abc123"
  }
}
```

## Implementation Plan

### Phase 5 (Steering) — Intent Surfacing (Completed)

1. Add pending_intents field to SteeringBlock ✅
2. Implement surfacing logic based on urgency/scope ✅
3. Update exo status to show relevant pending intents ✅
4. Wire steering output to agent prompts ✅

### Phase 7a (Inbox Migration) — Intent Schema

1. Define inbox.toml schema with intent categories
2. Migration: rename feedback.toml → inbox.toml
3. Update Machine Channel protocol
4. Update Studio UI (already half-implemented)

### Migration via exo update

```bash
# In exo update command:
if exists("docs/agent-context/feedback.toml") {
    rename("feedback.toml", "inbox.toml")
    log("Migrated feedback.toml → inbox.toml")
}
```

## Compatibility

- **Breaking**: feedback.toml renamed to inbox.toml
- **Migration**: Automatic via exo update
- **Existing feedback**: Preserved, just renamed

## Alternatives Considered

### 1. Keep feedback.toml name

Rejected: "feedback" is too passive. "Inbox" better captures the queue semantics.

### 2. Store intents in chat context

Rejected: Chat context compacts. Intents need persistence.

### 3. Separate intent file per category

Rejected: Overcomplication. Single inbox with category field is simpler.

## Open Questions

_All resolved during RFC review:_

1. ~~Should resolved intents be deleted or archived?~~ → **Archived** (audit trail)
2. ~~How long to keep archived intents?~~ → **30 days**, then exo gc cleans
3. ~~Should agents auto-acknowledge or require explicit ack?~~ → **Auto-ack** on first steering read
4. ~~Integration with GitHub Issues for external tracking?~~ → **Future work**, not MAP-critical

## References

- RFC 0016: Attention is Sovereign (superseded by this RFC)
- steering.rs: Current steering implementation
- feedback.toml schema: Current prototype schema

---

## UI Integration (VS Code Extension)

### User-Facing Commands

The extension provides these commands for intent management:

| Command                      | Keybinding    | Description                        |
| ---------------------------- | ------------- | ---------------------------------- |
| `exosuit.captureIntent`      | `Cmd+Shift+I` | Quick capture intent modal         |
| `exosuit.openInboxQuickPick` | -             | Show pending intents in Quick Pick |
| `exosuit.focusInbox`         | -             | Open inbox tree view / webview     |
| `exosuit.resolveIntent`      | -             | Mark intent as resolved            |

### Status Bar Indicator

A status bar item displays the count of active intents:

- **Hidden**: When count = 0
- **Badge**: `$(inbox) 3` when count > 0
- **Tooltip**: "3 pending intents - click to review"
- **Click behavior**: Opens Quick Pick

### Quick Capture Flow

1. User presses `Cmd+Shift+I` while editing code
2. Extension detects:
   - Current file → scope (e.g., `rust`, `typescript`)
   - Selected text → pre-fill subject
   - Active phase → scope context
3. Modal sequence:
   - Subject (pre-filled)
   - Category (QuickPick)
   - Urgency (QuickPick)
   - Body (optional)
4. Extension calls `exo inbox add` via CLI
5. Confirmation toast
6. Status bar updates immediately

### Agent Integration

A new LM tool `exo-inbox` (zero-arg) allows agents to:

- List pending intents
- User sees acknowledgment in UI (status change)

### Notification Strategy

Following "Attention is Sovereign":

- **No interrupting toasts** for new intents
- **Passive indicator**: Status bar badge
- **User-initiated review**: Click to triage

---

## Machine Channel Wire Format

### Add Intent

```json
// Request
{
  "op": "intent",
  "action": "add",
  "category": "correction",
  "subject": "Use snake_case",
  "body": "Rust identifiers should use snake_case...",
  "scope": "rust",
  "urgency": "next-touch"
}

// Response (success)
{
  "status": "ok",
  "intent_id": "intent-01KE...",
  "message": "Intent added to inbox.toml"
}
```

### Resolve Intent

```json
// Request
{
  "op": "intent",
  "action": "resolve",
  "id": "intent-01KE...",
  "resolution": "Applied snake_case in commit abc123"
}

// Response
{
  "status": "ok",
  "message": "Intent resolved"
}
```

---

## Implementation Checklist

### Phase 7a (Infrastructure) ✅

- [x] Verify steering integration surfaces intents correctly
- [x] Implement `exo inbox archive` command
- [x] Add archive system (`exo gc inbox` with 30-day default)
- [x] Update appendix to reflect actual implementation status

### Phase 7b (UI MVP)

- [ ] Command: `exosuit.captureIntent` with keybinding
- [ ] Service: InboxStatusBarService (watcher + badge)
- [ ] Command: `exosuit.openInboxQuickPick`
- [ ] LM Tool: `exo-inbox` for agent access
- [ ] Remove broken FeedbackSidebar (replaced by inbox UI)

### Phase 7c (Ideas Workflow)

- [ ] Create RFC 0061: Ideas Workflow (or update existing)
- [ ] Implement `exo idea triage` command
- [ ] Implement `exo idea promote` command
- [ ] Integrate ideas into `exo map` steering

---

## Appendix: Current Infrastructure Audit

### What Exists (Phase 7a Complete ✅)

1. **inbox.toml** - Intent schema with full lifecycle tracking
2. **Steering integration** - `get_surfaced_intents()` surfaces intents based on urgency/scope
3. **CLI Commands**:
   - `exo inbox list/add/ack/resolve/archive` - Full intent lifecycle
   - `exo gc inbox [--days N]` - Archive cleanup (default 30 days)
4. **Machine Channel** - Supports feedback/intent operation types

### What's Missing (Phase 7b/7c)

1. **VS Code UI** - Status bar, Quick Pick, keybindings (Phase 7b)
2. **LM Tool** - `exo-inbox` for agent access (Phase 7b)
3. **Ideas integration** - Triage and promote workflows (Phase 7c)
