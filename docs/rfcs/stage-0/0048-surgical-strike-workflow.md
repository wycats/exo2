<!-- exo:48 ulid:01ky5qdmtmyy6sr43pct5p5j17 -->

---
ulid: 01kg5kp2da62rwfsxhsg75emsa
title: Surgical Strike Workflow
feature: Workflow
superseded_by: "10175"
---


# RFC 0048: Surgical Strike Workflow

- **Superseded by**: RFC 10175

## Summary

This RFC proposes a "Stack-Based" workflow for the Exosuit Agent, allowing it to pause the current "Main Phase" to execute a high-focus, isolated task ("Surgical Strike") and then pop back to the main context with zero loss of continuity.

## Motivation

### The Problem: Context Drift

During a long-running phase (e.g., "Implement Feature X"), the Agent often encounters blocking issues unrelated to the main goal (e.g., "The build script is broken", "The linter is misconfigured").

Currently, the Agent has two bad options:

1.  **Pollute the Phase**: Fix the script inside the current phase. This muddies the `implementation-plan.toml` and `plan.toml` with irrelevant tasks.
2.  **Abandon the Phase**: Finish the current phase prematurely to start a "Fix Script" phase. This breaks the narrative arc of the feature.

### The Solution: The Call Stack

We introduce a **Phase Stack**.

- **Bottom**: The Main Phase (e.g., "Feature X").
- **Top**: The Active Surgical Strike (e.g., "Fix Build Script").

When a Strike is active, the Agent's "Context" is temporarily narrowed to the Strike. When finished, the Strike is popped, and the Main Phase resumes.

## Detailed Design

### 1. Data Model

We modify `docs/agent-context/plan.toml` to support a stack.

```toml
# plan.toml

[context]
# The main, long-running phase
phase_id = "0060"
phase_name = "Reactive Architecture"

# The stack of active interruptions (Last-In, First-Out)
[[context.surgical_strikes]]
id = "strike-1"
name = "Fix Bootstrap Script"
goal = "Ensure bootstrap.sh handles read-only files correctly"
started_at = "2025-12-10T10:00:00Z"
# The strike has its own mini-plan
tasks = [
  { title = "Update script", status = "completed" },
  { title = "Verify permissions", status = "in-progress" }
]
```

### 2. CLI Commands

We add a new `exo strike` subcommand group.

- **`exo strike start <name>`**:
  - Pushes a new Strike onto the stack.
  - Updates `plan.toml`.
  - **Effect**: The "Active Goal" becomes this Strike.
- **`exo strike status`**:
  - Shows the status of the _current_ strike.
- **`exo strike finish`**:
  - Verifies the Strike's tasks are done.
  - Pops the Strike from the stack.
  - **Effect**: The "Active Goal" reverts to the Main Phase.
- **`exo strike abort`**:
  - Discards the Strike without marking it complete.

### 3. UI Behavior (`exo phase status`)

The `exo phase status` command must adapt to the stack.

**Principle**: "Focus on the Active Frame."

If a Surgical Strike is active:

1.  **Primary Output**: Display the Strike's status (Goal, Tasks, Progress) in full detail.
2.  **Secondary Output**: Display a _terse summary_ of the Main Phase (e.g., "Main Phase: 'Reactive Architecture' (Paused)").
3.  **Guidance**: Include a hint: `(Use --full to see main phase details)`.

**Example Output**:

```text
$ exo phase status

🔴 SURGICAL STRIKE: Fix Bootstrap Script
========================================
Goal: Ensure bootstrap.sh handles read-only files correctly
Started: 10 mins ago

Tasks:
[x] Update script
[ ] Verify permissions

----------------------------------------
Main Phase: [0060] Reactive Architecture (Background)
(Use --full to view main phase context)
```

### 4. Integration with `exo task`

- `exo task add` adds to the **Active Frame** (the top of the stack).
- If a Strike is active, tasks go there.
- If no Strike is active, tasks go to the Main Phase.

## Implementation Plan

1.  **Update Rust Structs**: Modify `Context` and `Plan` structs in `tools/exo/src/` to include `surgical_strikes: Vec<SurgicalStrike>`.
2.  **Update TOML Serialization**: Ensure `plan.toml` can serialize/deserialize this stack.
3.  **Implement CLI**: Add `strike` subcommand to `main.rs`.
4.  **Update Status Logic**: Modify `print_status` to check the stack depth and render accordingly.
