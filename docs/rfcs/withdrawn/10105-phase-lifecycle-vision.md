<!-- exo:10105 ulid:01kmzxey0rsy63ahf174ar2m87 -->


# RFC 10105: Phase Lifecycle Vision

---
status: provisional
---

# North Star User Journey: The Phase Lifecycle

## Context

This document zooms in on the "Deep Work Loop" and "Gatekeeper" chapters of the main North Star journey. It details the ideal experience of moving through a single phase of development within Exosuit.

## The Lifecycle

### 1. Phase Initialization (The Setup)

**Goal**: Orient the user and agent for the work ahead.

1.  **Trigger**:
    - **New Phase**: User clicks **[ Start Phase X ]**.
    - **Resume Phase**: User clicks **[ Resume Phase ]** (if a phase is already in progress but the chat is fresh).
2.  **System Action**:
    - **Start**: Archives previous phase, scaffolds `current/`, updates `plan-outline.md`.
    - **Resume**: Restores the "Active Context" from `docs/agent-context/current/` into the chat session.
3.  **Interaction**:
    - **Agent**: "Phase X started/resumed. I see the goal is [Goal]. Shall we draft/continue the Implementation Plan?"

### 2. Planning & Alignment (The Blueprint)

**Goal**: Agree on _what_ to build before building it.

1.  **Drafting**:
    - User and Agent co-author `implementation-plan.md`.
    - Agent suggests tasks based on the plan and populates `task-list.md`.
2.  **Review**:
    - User reviews the plan in the "Current Phase" pane.
    - User approves the plan.

### 3. Execution (The Loop)

**Goal**: Complete tasks with high focus and coherence.

1.  **Task Selection (Setting the Stage)**:
    - User clicks "Play" on a task in the sidebar.
    - **System**:
      - Updates "Active Context" (loads relevant files/docs for that task).
      - Signals to the Agent: "The user is now focusing on Task X."
2.  **Mid-Phase Check-in**:
    - **Trigger**: User clicks **[ Status Report ]**.
    - **System**: Compares `task-list.md` vs. `walkthrough.md` vs. Codebase.
    - **Agent**: "We are 50% through. Task X is done, but Task Y is blocked by Z. Shall we pivot?"
3.  **Implementation**:
    - User/Agent write code.
    - Agent checks `axioms.md` (for permanent constraints) and `docs/rfcs/*` to ensure alignment.
4.  **Completion**:
    - User marks task as complete.
    - **System**: Automatically appends a summary to `walkthrough.md` with a `<!-- status: pending-review -->` flag.
    - **User**: Can review/edit the summary later in the "Pending Reviews" section.

### 4. Verification (The Gate)

**Goal**: Ensure quality and consistency before moving on.

1.  **Trigger**: User clicks **[ Verify Phase ]**.
2.  **System Action**:
    - **Instant Check**: Queries VS Code's `getDiagnostics()` API for immediate feedback on lint/compile errors (0s latency).
    - **Deep Check**: Runs automated test suites and custom scripts in `scripts/agent/checks/`.
    - **Coherence Check**: Verifies alignment between docs and code.
    - Generates a "Phase Report".
3.  **Resolution**:
    - If failures exist, User fixes them and re-verifies.

### 5. Transition (The Handoff)

**Goal**: Cleanly close the current context and prepare for the next.

1.  **Trigger**: User clicks **[ Complete Phase ]**.
2.  **Incomplete Task Handling**:
    - **System**: Detects unchecked items in `task-list.md`.
    - **Wizard**: "You have 3 incomplete tasks. [Defer to Next Phase] [Move to Backlog] [Mark as Won't Do]".
3.  **System Action**:
    - Compresses `walkthrough.md` into a summary for the next phase.
    - Updates `changelog.md`.
    - Prompts user to update `docs/rfcs/*` if new patterns emerged.
    - Archives the phase.
4.  **Preparation (Optional)**:
    - **Trigger**: User clicks **[ Prepare Next Phase ]** (if continuing in same session).
    - **System**: Displays the `future/` items and prompts for a high-level outline of the next phase.
5.  **Result**: Ready for Phase X+1.
