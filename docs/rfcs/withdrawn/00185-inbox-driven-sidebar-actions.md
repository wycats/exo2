<!-- exo:185 ulid:01kmzxefdshw7a0bzhrvfgrb5p -->

# RFC 185: Inbox-Driven Sidebar Actions

- **Superseded by**: RFC 0124

- **Status**: Withdrawn
- **Stage**: 1
- **Reason**:

# RFC 00185: Inbox-Driven Sidebar Actions

> **WITHDRAWN**: This RFC has been absorbed into [RFC 0124: Inbox System](../archive/0124-async-intent-channel.md). The "sidebar is a channel, not a backdoor" principle and action types are now documented there.

## Summary

Route sidebar action buttons (Complete Goal, Add Notes, Verify Task) through the inbox system instead of directly calling CLI commands. This ensures the agent is aware of all user actions and can respond appropriately.

## Motivation

### The Problem

Sidebar buttons currently bypass the agent by directly calling CLI commands:

```typescript
// Current: Direct mutation
await exec(`exo goal complete "${goalId}"`);
await exec(`exo task complete "${taskId}"`);
await exec(`exo impl add-task-log "${id}" "${note}"`);
```

This creates several issues:

1. **Agent blindness** — The agent doesn't know the user took action
2. **Lost context** — User's intent (why they completed, what they verified) isn't captured
3. **No feedback loop** — Agent can't ask clarifying questions or suggest follow-ups
4. **Inconsistent model** — Sidebar is a "backdoor" rather than a communication channel

### The Insight

The sidebar should be a **channel to the agent**, not a backdoor to mutate state. User actions are **intents** that the agent should process, potentially with additional context or follow-up.

## Proposal

### 1. Route Actions Through Inbox

Instead of calling CLI directly, sidebar buttons add intents to `inbox.toml`:

```toml
[[intent]]
id = "intent-01KG..."
created = "2026-02-01T10:30:00Z"
status = "pending"
category = "priority"  # User is prioritizing this action
subject = "Complete goal: Implement sidebar routing"
subject_ref = "goal:01KGABC123..."  # CanonicalRef targeting
scope = { phase = "01KGC16R2G..." }
urgency = "immediate"

[intent.action]
type = "complete-goal"
evidence = "All tasks verified, tests passing"
```

### 2. Action Types

New `[intent.action]` field on intents for structured actions:

| Action Type     | Fields     | Description               |
| --------------- | ---------- | ------------------------- |
| `complete-goal` | `evidence` | Mark goal as complete     |
| `complete-task` | `evidence` | Mark task as complete     |
| `verify-task`   | `evidence` | Verify task with evidence |
| `add-note`      | `note`     | Add note to item          |

### 3. UI Feedback Badge

Items with pending intents show a "feedback waiting" indicator:

```typescript
// In TreeDataService
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
    return new vscode.ThemeIcon("comment-unresolved"); // Or custom badge
  }
  return getDefaultIcon(itemId);
}
```

### 4. Steering Integration

`pending_intents` in steering response includes `subject_ref`:

```json
{
  "pending_intents": [
    {
      "id": "intent-01KG...",
      "category": "priority",
      "subject": "Complete goal: Implement sidebar routing",
      "subject_ref": "goal:01KGABC123...",
      "relevance": 1.0,
      "action": {
        "type": "complete-goal",
        "evidence": "All tasks verified"
      }
    }
  ]
}
```

### 5. Agent Processing

When agent sees an action intent:

1. **Acknowledge** — Mark intent as acknowledged
2. **Validate** — Check if action is appropriate (e.g., are all tasks done?)
3. **Execute** — Call the underlying CLI command
4. **Resolve** — Mark intent as resolved with outcome

This allows the agent to:

- Ask clarifying questions before acting
- Suggest additional actions
- Maintain awareness of user activity

## Implementation Plan

### Phase 1: Schema Extension

- [ ] Add `subject_ref` to `Intent` struct in `tools/exo/src/inbox/mod.rs`
- [ ] Add `action` field to `Intent` struct
- [ ] Add `subject_ref` to `SurfacedIntent` struct
- [ ] Update `exo inbox add` to accept `--subject-ref` and `--action` flags
- [ ] Update `inbox.toml` template

### Phase 2: Steering Integration

- [ ] Include `subject_ref` in `exo status --format json` output
- [ ] Add `action` field to steering JSON response
- [ ] Update `exo-steering` tool to surface action intents

### Phase 3: UI Button Routing

- [ ] Modify `exosuit.completeGoal` to add intent instead of calling CLI
- [ ] Modify `exosuit.addTaskNote` to add intent instead of calling CLI
- [ ] Add confirmation toast: "Action queued for agent"

### Phase 4: Feedback Badges

- [ ] Extend `TreeDataService` to accept `pendingIntents` parameter
- [ ] Compute "has feedback" state at render time
- [ ] Show badge/decoration on items with pending intents
- [ ] Clear badge when intent is resolved

## Migration Notes

### Existing Buttons

The current direct-call behavior should be preserved as a fallback:

```typescript
// If inbox routing fails, fall back to direct call
try {
  await addInboxIntent({ ... });
} catch (e) {
  logger.warn("Inbox routing failed, falling back to direct call");
  await exec(`exo goal complete "${goalId}"`);
}
```

### User Expectation

Users expect immediate feedback. The UI should:

1. Show "Action queued" toast immediately
2. Show badge on the item
3. Refresh tree when agent resolves the intent

## Compatibility

- **Non-breaking**: Existing CLI commands unchanged
- **Additive**: New `subject_ref` and `action` fields are optional
- **Graceful degradation**: Falls back to direct calls if inbox fails

## Alternatives Considered

### Option B: Event Bus

Route actions through a VS Code event bus that both UI and agent subscribe to.

**Rejected**: Adds complexity without persistence. Intents would be lost on extension reload.

### Option C: Direct Agent Invocation

Call the agent directly via chat API when button is clicked.

**Rejected**: Requires active chat session. Inbox approach works asynchronously.

## References

- RFC 0124: Async Intent Channel (parent RFC)
- RFC 0016: Attention is Sovereign (philosophical foundation)
