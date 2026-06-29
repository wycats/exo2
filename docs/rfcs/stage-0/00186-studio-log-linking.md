<!-- exo:186 ulid:01kmzxeffgzfr8ye4xex63w088 -->

# RFC 186: Studio Log Linking


# RFC 00186: Studio Log Linking

## Summary

Add click-to-focus behavior for completion log entries in Phase Details, allowing users to navigate from a log entry in the sidebar to its context in Studio.

## Motivation

Completion logs appear under goals and tasks in the Phase Details tree view, showing when items were completed and with what notes. Currently these entries are display-only—users can see the log but cannot navigate to see it in context within Studio.

This creates a disconnect: the sidebar shows _what_ happened, but users must manually find _where_ it happened in the implementation plan.

## Design

### Approach

Extend the existing `FocusRequest` mechanism to support focusing on goals/tasks, not just phases.

### FocusRequest Extension

Current:

```typescript
type FocusRequest = {
  kind: "phase";
  id: string;
  requestId: number;
};
```

Proposed:

```typescript
type FocusRequest = {
  kind: "phase" | "goal" | "task" | "log";
  id: string;
  parentId?: string; // For logs: the goal or task ID
  requestId: number;
};
```

### Tree Item Command

Add a command to completion log tree items in `TreeDataService.ts`:

```typescript
const buildCompletionLogItems = (parentId: string, lines: string[]) => {
  return lines.map((line, index) => {
    const item = new ExosuitTreeItem(/* ... */);
    item.id = `${parentId}/log/${index + 1}`;
    item.command = {
      command: "exosuit.focusLog",
      title: "Focus Log Entry",
      arguments: [parentId, index + 1],
    };
    return item;
  });
};
```

### New Command: `exosuit.focusLog`

Register a command that:

1. Opens the implementation plan in Studio
2. Sends a FocusRequest with `kind: "log"` and the parent goal/task ID
3. Studio scrolls to and highlights the parent goal/task

### Studio Handling

Extend the focus effect in Studio:

```typescript
$effect(() => {
  if (!focus) return;
  if (focus.requestId === lastFocusRequestId) return;
  lastFocusRequestId = focus.requestId;

  if (focus.kind === "phase") {
    // existing phase focus logic
  } else if (focus.kind === "goal" || focus.kind === "log") {
    // Scroll to goal element
    const goalEl = document.querySelector(
      `[data-goal-id="${focus.parentId || focus.id}"]`,
    );
    if (goalEl) {
      goalEl.scrollIntoView({ behavior: "smooth", block: "start" });
    }
  }
});
```

### Scope Boundaries

**In scope:**

- Click-to-focus from Phase Details completion logs
- Scrolling to parent goal/task in Studio

**Out of scope:**

- Rendering completion logs inline in Studio (separate enhancement)
- Editing completion logs from Studio
- Deep linking to specific log lines within multi-line logs

## Implementation Plan

1. Extend `FocusRequest` type in `packages/exosuit-vscode/src/types/`
2. Add `exosuit.focusLog` command in `extension.ts`
3. Update `TreeDataService.ts` to add command to completion log items
4. Update Studio focus handling to support `goal`/`log` kinds
5. Add `data-goal-id` attributes to goal elements in Studio

## Alternatives

1. **Focus on parent phase instead**: Simpler but less precise—user still has to find the goal manually
2. **Open `implementation-plan.toml` in text editor**: Loses the Studio UX benefits

## References

- RFC 00184: Mode-Aware Sidebar Cockpit (related sidebar work)

