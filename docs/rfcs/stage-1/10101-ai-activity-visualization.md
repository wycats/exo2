<!-- exo:10101 ulid:01kmzxey0wwpc1vtm8sfy27pj5 -->


# RFC 10101: AI Activity Visualization

- **Superseded by**: RFC 0008


## Goal

Visualize the internal thought process and context gathering of the `@exosuit` agent to build trust and aid debugging.

## Design Philosophy: "Native Glass Box"

The visualization should feel like a native part of VS Code, not a flashy external tool.

- **Icons**: Use VS Code [Codicons](https://microsoft.github.io/vscode-codicons/dist/codicon.html) instead of emojis.
- **Typography**: Use standard VS Code fonts and colors.
- **Layout**: Compact, timeline-based list.

## Data Model

We will transition `LogService` from storing strings to storing structured `ActivityEvent` objects.

```typescript
interface ActivityEvent {
  id: string;
  timestamp: number;
  type: "system" | "context" | "axiom" | "llm";
  label: string;
  details?: string;
  icon?: string; // e.g., "file-text", "search", "lightbulb"
}
```

## UI Representation

### The "Activity" Panel

A Webview (or TreeView) that renders the timeline.

**Visual Style:**

```text
[12:01:45] $(hubot) Processing Request...
  │
  ├─ $(file-text) Read: plan-outline.md
  ├─ $(file-text) Read: current/task-list.md
  │
  ├─ $(search) Axiom Search: "Assess button"
  │  └─ $(check) Match: Axiom 5 (Score: 0.89)
  │
  └─ $(sparkle) Response Generated (4k tokens)
```

## Implementation Plan

1.  **LogService**: Update to support `logActivity(event: ActivityEvent)`.
2.  **ExosuitChat**: Instrument the `getProjectContext` flow to emit events.
3.  **DebugLogProvider**: Rename to `ActivityLogProvider` and update rendering logic to use Codicons and structured HTML.
