<!-- exo:54 ulid:01kg5kp2dkqce6s6z7b144me5s -->

# RFC 54: The Context Inbox (Pull-Based Attention)

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**:

# RFC 0054: The Context Inbox

## Summary

This RFC proposes replacing the intrusive "focus-stealing" behavior of automated file creation with a passive **Context Inbox**. This system queues new artifacts (RFCs, Plans, Reports) for user review, allowing the user to "pull" context when ready rather than having it "pushed" onto their active stack.

## Motivation

The current implementation of the `exo` CLI automatically opens newly created files (e.g., RFCs) in the editor. While intended to be helpful, this behavior:
1.  **Breaks Flow**: It forcibly context-switches the user, interrupting their current train of thought.
2.  **Violates Agency**: It assumes the user wants to process the new information *immediately*.
3.  **Creates Anxiety**: Multiple automated actions can lead to a flurry of opening tabs, overwhelming the workspace.

We need a mechanism that respects the user's cognitive state while ensuring new information is not lost.

## The Axiom: Attention is Sovereign

This RFC establishes a new core axiom for the Exosuit User Experience:

> **Attention is Sovereign**
> The user's focus is a sacred resource. The system must never forcibly redirect attention unless system integrity is critical (e.g., "Red Entropy"). All non-critical context updates must be queued for voluntary ingestion.

## Detailed Design

### 1. The Inbox Protocol (`inbox.toml`)

We introduce a new context file: `docs/agent-context/current/inbox.toml`. This file acts as a persistent queue for user-facing notifications and artifacts.

**Schema:**
```toml
[[item]]
id = "uuid-v4"
type = "rfc" # or "plan", "report", "alert"
title = "RFC 0016: The Context Inbox"
path = "docs/rfcs/stage-0/0041-context-inbox.md"
created_at = "2025-12-12T10:00:00Z"
priority = "normal" # "low", "normal", "high"
status = "unread"
context_id = "phase-25" # Optional: Links item to a specific context
```

### 2. CLI Behavior Change

The `exo` CLI (specifically `tools/exo`) will be modified to stop invoking the `$EDITOR` directly.

*   **Old Behavior**: Generate file -> `Command::new(editor).arg(path).spawn()`
*   **New Behavior**: Generate file -> Append entry to `inbox.toml` -> Print `[Inbox] Added: <filename>` to stdout.

### 3. The "Inbox" Experience (VS Code)

The Exosuit VS Code extension will serve as the primary consumer of the Inbox.

*   **Watcher**: The extension watches `docs/agent-context/current/inbox.toml`.
*   **Status Bar**: A subtle indicator showing the count of unread items (e.g., `$(bell) 3`).
    *   *Zero items*: Hidden or dimmed.
    *   *New items*: Visible.
*   **Interaction**:
    *   Clicking the status bar item invokes `exosuit.openInbox`.
    *   **QuickPick / Palette**: Shows a list of unread items.
    *   **Selection**:
        1.  Opens the file.
        2.  Removes the item from `inbox.toml` (or marks it read).

### 4. Lateral Thinking: The "Desk" Metaphor

We are moving from a "Pop-up" metaphor to a "Desk" metaphor.
*   **The Editor**: Your immediate workspace.
*   **The Inbox**: A physical tray on the corner of the desk.
*   **The Agent**: A colleague who walks up, places a document in the tray, and walks away silently, trusting you to read it when you are ready.

### 5. Integration with Modal Workflows (RFC 0029)

The Inbox serves as the "Content Layer" for the Modal Workflow system.

*   **Signal vs. Content**: The **Maker HUD** (RFC 0029) provides the *Signal* (e.g., Red Entropy Indicator). The **Inbox** provides the *Content* (e.g., "Critical Axiom Violation").
*   **Hermetic Seal**: The Inbox never "leaks" content (popups/toasts) into Maker Mode. The user must explicitly "pull" the content by clicking the HUD/Inbox.
*   **Gating**:
    *   **In Flow**: The Inbox is purely passive.
    *   **Phase Transition**: The CLI checks for "Critical" Inbox items before allowing a Phase Finish.
    *   **Epoch Transition**: A "Hard Gate" requires the Inbox to be cleared of critical debt.

### 6. Contextual Evaporation (Hygiene)

To prevent "Inbox Bankruptcy," items are context-bound.

*   **Mechanism**: When the system detects a Context Switch (e.g., `exo phase finish`), it scans `inbox.toml`.
*   **Evaporation**: Items linked to the completed context (via `context_id`) are automatically removed or archived.
*   **Rationale**: An alert about "Phase 4 Plan Mismatch" is irrelevant once Phase 4 is finished.

## Alternatives Considered

*   **Toast Notifications**: Rejected. Toasts are ephemeral and can be missed if the user is away. They also still demand visual attention.
*   **Terminal Hyperlinks**: Rejected. While useful, they scroll away and don't provide a persistent "To Read" state.
*   **"Just don't open it"**: Rejected. Without a notification mechanism, the user might forget they created the file or lose track of where it is.

## Implementation Plan (Stage 2)

1.  [ ] Define `InboxItem` struct in `exosuit-core`.
2.  [ ] Update `exo` CLI to write to `inbox.toml` instead of opening files.
3.  [ ] Implement `InboxService` in `exosuit-vscode` to watch the file.
4.  [ ] Add Status Bar item and `openInbox` command.


